use crate::config::{AiAnalysisSettings, AiFilterSettings, AiSettings, AiTranslationSettings};
use crate::error::{Result, TrendRadarError};
use crate::model::NewsItem;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static AI_BOLD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").unwrap());

// ============================================================================
// ChatMessage
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: &str) -> Self {
        ChatMessage {
            role: "system".to_string(),
            content: content.to_string(),
        }
    }

    pub fn user(content: &str) -> Self {
        ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        }
    }
}

// ============================================================================
// AI 客户端 (LiteLLM / OpenAI 兼容接口)
// ============================================================================

#[derive(Debug, Clone)]
pub struct AiClient {
    model: String,
    api_key: Option<String>,
    api_base: String,
    max_tokens: u32,
    temperature: f64,
    timeout_secs: u64,
    fallback_models: Vec<String>,
    http_client: reqwest::Client,
}

fn resolve_api_base(model: &str, explicit_base: Option<&str>) -> String {
    if let Some(base) = explicit_base {
        if !base.is_empty() {
            return base.to_string();
        }
    }
    let provider = model.split('/').next().unwrap_or("");
    match provider {
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        "openai" => "https://api.openai.com/v1".to_string(),
        "anthropic" => "https://api.anthropic.com/v1".to_string(),
        "gemini" | "google" => "https://generativelanguage.googleapis.com/v1beta".to_string(),
        "ollama" => "http://localhost:11434/v1".to_string(),
        "openrouter" => "https://openrouter.ai/api/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    }
}

fn resolve_model_name(model: &str) -> String {
    if let Some(idx) = model.find('/') {
        model[idx + 1..].to_string()
    } else {
        model.to_string()
    }
}

impl AiClient {
    pub fn new(settings: &AiSettings) -> Result<Self> {
        let raw_model = settings
            .model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o-mini".to_string());
        let api_base = resolve_api_base(&raw_model, settings.api_base.as_deref());
        let model_name = resolve_model_name(&raw_model);

        let timeout_secs = settings.timeout.unwrap_or(120);

        Ok(AiClient {
            model: model_name,
            api_key: settings.api_key.clone(),
            api_base,
            max_tokens: settings.max_tokens.unwrap_or(4096),
            temperature: settings.temperature.unwrap_or(0.7),
            timeout_secs,
            fallback_models: Vec::new(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .map_err(|e| TrendRadarError::Ai(format!("Failed to build HTTP client: {}", e)))?,
        })
    }

    pub fn with_fallback(mut self, models: Vec<String>) -> Self {
        self.fallback_models = models;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let models: Vec<&String> = std::iter::once(&self.model)
            .chain(self.fallback_models.iter())
            .collect();

        let mut last_error = String::new();

        for model in &models {
            match self.try_chat(messages, model).await {
                Ok(content) => return Ok(content),
                Err(e) => {
                    last_error = format!("{}", e);
                    println!("AI 模型 {} 请求失败: {}，尝试下一个...", model, last_error);
                }
            }
        }

        Err(TrendRadarError::Ai(format!(
            "所有 AI 模型请求均失败: {}",
            last_error
        )))
    }

    async fn try_chat(&self, messages: &[ChatMessage], model: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));

        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
        });

        let mut req = self.http_client.post(&url).json(&body);

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req.send().await.map_err(|e| {
            TrendRadarError::Ai(format!("HTTP request failed: {}", e))
        })?;

        let status = resp.status();
        let resp_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(TrendRadarError::Ai(format!(
                "AI API 返回错误 {}: {}",
                status, resp_text
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&resp_text)
            .map_err(|e| TrendRadarError::Ai(format!("JSON parse error: {}", e)))?;

        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| {
                parsed["choices"][0]["message"]["content"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
            })
            .ok_or_else(|| {
                TrendRadarError::Ai(format!(
                    "无法从 AI 响应中提取内容: {}",
                    resp_text
                ))
            })?;

        Ok(content.to_string())
    }

    pub async fn chat_json<T: serde::de::DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
    ) -> Result<T> {
        let response = self.chat(messages).await?;
        let cleaned = extract_json_from_response(&response);
        serde_json::from_str(&cleaned).map_err(|e| {
            TrendRadarError::Ai(format!(
                "JSON 解析失败: {}\n原始响应: {}",
                e, response
            ))
        })
    }

    pub async fn chat_json_with_retry<T: serde::de::DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
        max_retries: usize,
    ) -> Result<T> {
        let mut last_err = String::new();
        for attempt in 0..=max_retries {
            if attempt > 0 {
                let wait = 2u64.pow(attempt as u32);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
            let response = match self.chat(messages).await {
                Ok(r) => r,
                Err(e) => {
                    last_err = format!("{}", e);
                    continue;
                }
            };
            if response.trim().is_empty() || response.trim() == "," {
                last_err = format!("AI 返回空内容 (尝试 {}/{})", attempt, max_retries);
                continue;
            }
            let cleaned = extract_json_from_response(&response);
            match serde_json::from_str::<T>(&cleaned) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    last_err = format!("JSON 解析失败 (尝试 {}/{}): {} 原始: {}...",
                        attempt, max_retries, e,
                        &response[..response.len().min(200)]);
                }
            }
        }
        Err(TrendRadarError::Ai(last_err))
    }

    pub async fn validate(&self) -> Result<()> {
        if self.model.is_empty() {
            return Err(TrendRadarError::Ai(
                "模型名不能为空".to_string(),
            ));
        }
        if self.api_base.is_empty() {
            return Err(TrendRadarError::Ai(
                "API Base URL 不能为空".to_string(),
            ));
        }
        Ok(())
    }
}

fn extract_json_from_response(text: &str) -> String {
    let text = text.trim();

    let trimmed = text.trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Some(start) = trimmed.find('{') {
        let substring = &trimmed[start..];
        if let Some(end) = substring.rfind('}') {
            return substring[..=end].to_string();
        }
    }

    if let Some(start) = trimmed.find('[') {
        let substring = &trimmed[start..];
        if let Some(end) = substring.rfind(']') {
            return substring[..=end].to_string();
        }
    }

    trimmed.to_string()
}

// ============================================================================
// 提示词加载器
// ============================================================================

pub fn load_prompt_template(file_path: &str) -> Result<(String, String)> {
    let content = std::fs::read_to_string(file_path)
        .map_err(|e| TrendRadarError::Ai(format!("无法加载提示词文件 {}: {}", file_path, e)))?;

    let mut system_prompt = String::new();
    let mut user_prompt = String::new();
    let mut current_section = "";

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[system]") {
            current_section = "system";
        } else if trimmed.eq_ignore_ascii_case("[user]") {
            current_section = "user";
        } else if !trimmed.is_empty() || current_section == "user" {
            match current_section {
                "system" => {
                    if !system_prompt.is_empty() {
                        system_prompt.push('\n');
                    }
                    system_prompt.push_str(line);
                }
                "user" => {
                    if !user_prompt.is_empty() {
                        user_prompt.push('\n');
                    }
                    user_prompt.push_str(line);
                }
                _ => {}
            }
        }
    }

    Ok((system_prompt, user_prompt))
}

// ============================================================================
// AI 分析结果数据模型
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiAnalysisResult {
    #[serde(default)]
    pub core_trends: serde_json::Value,
    #[serde(default)]
    pub sentiment_controversy: serde_json::Value,
    #[serde(default)]
    pub signals: serde_json::Value,
    #[serde(default)]
    pub rss_insights: serde_json::Value,
    #[serde(default)]
    pub outlook_strategy: serde_json::Value,
    #[serde(default)]
    pub standalone_summaries: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StandaloneSummary {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
}

// ============================================================================
// AI 分析器
// ============================================================================

pub struct AiAnalyzer {
    client: AiClient,
    system_prompt: String,
    user_prompt_template: String,
    max_news_for_analysis: usize,
}

impl AiAnalyzer {
    pub fn new(
        settings: &AiAnalysisSettings,
        global_ai: Option<&AiSettings>,
        system_prompt: String,
        user_prompt_template: String,
    ) -> Result<Self> {
        let ai_settings = AiSettings {
            provider: settings.provider.clone().or_else(|| global_ai.and_then(|g| g.provider.clone())),
            model: settings.model.clone().or_else(|| global_ai.and_then(|g| g.model.clone())),
            api_key: settings.api_key.clone().or_else(|| global_ai.and_then(|g| g.api_key.clone())),
            api_base: settings.api_base.clone().or_else(|| global_ai.and_then(|g| g.api_base.clone())),
            max_tokens: Some(global_ai.and_then(|g| g.max_tokens).unwrap_or(8192)),
            temperature: Some(global_ai.and_then(|g| g.temperature).unwrap_or(0.7)),
            timeout: Some(global_ai.and_then(|g| g.timeout).unwrap_or(120)),
        };
        Ok(AiAnalyzer {
            client: AiClient::new(&ai_settings)?,
            system_prompt: system_prompt.trim().to_string(),
            user_prompt_template: user_prompt_template.trim().to_string(),
            max_news_for_analysis: settings.max_news_for_analysis.unwrap_or(200),
        })
    }

    pub async fn analyze(
        &self,
        news_items: &[NewsItem],
        rss_items: &[crate::model::RssItem],
        query_time: &str,
    ) -> Result<AiAnalysisResult> {
        let news_content = self.format_news_content(news_items);
        let rss_content = self.format_rss_content(rss_items);

        let mut user_prompt = self.user_prompt_template.clone();
        user_prompt = user_prompt.replace("{news_content}", &news_content);
        user_prompt = user_prompt.replace("{rss_content}", &rss_content);
        user_prompt = user_prompt.replace("{query_time}", query_time);

        let messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(&user_prompt),
        ];

        match self.client.chat_json(&messages).await {
            Ok(result) => Ok(result),
            Err(first_error) => {
                tracing::warn!("AI 分析 JSON 解析失败，尝试 AI 修复: {}", first_error);
                let raw_response = self.client.chat(&messages).await.ok();
                if let Some(raw) = raw_response {
                    match self.retry_fix_json(&raw).await {
                        Some(fixed) => {
                            tracing::info!("AI 修复 JSON 成功");
                            Ok(fixed)
                        }
                        None => Err(first_error),
                    }
                } else {
                    Err(first_error)
                }
            }
        }
    }

    async fn retry_fix_json(&self, raw_response: &str) -> Option<AiAnalysisResult> {
        let fix_prompt = format!(
            "上一次你的回复不是合法的 JSON 格式，请修复以下内容使其成为合法 JSON。\n\
             要求：\n\
             1. 输出必须是纯 JSON 对象，不要包含 markdown 代码块标记\n\
             2. 包含以下字段：core_trends, sentiment_controversy, signals, rss_insights, outlook_strategy\n\
             3. 每个字段可以是字符串、数组或对象\n\
             4. standalone_summaries 为对象，每个 key 为来源名称，value 为概括文本\n\n\
             原始回复：\n{}",
            raw_response
        );

        let messages = vec![
            ChatMessage::system("你是一个 JSON 修复助手。请将用户提供的文本修复为合法的 JSON 对象。只输出 JSON，不要其他内容。"),
            ChatMessage::user(&fix_prompt),
        ];

        self.client.chat_json(&messages).await.ok()
    }

    fn format_news_content(&self, items: &[NewsItem]) -> String {
        if items.is_empty() {
            return "暂无热点数据".to_string();
        }

        let items = if items.len() > self.max_news_for_analysis {
            &items[..self.max_news_for_analysis]
        } else {
            items
        };

        let mut output = String::new();
        for (i, item) in items.iter().enumerate() {
            let title = &item.title;
            let platform = &item.platform_name;
            let rank = item.rank.map_or("N/A".to_string(), |r| r.to_string());

            let count_info = if item.appearance_count > 1 {
                format!(" [出现{}次]", item.appearance_count)
            } else {
                String::new()
            };

            if item.rank == Some(0) {
                output.push_str(&format!(
                    "{}. [已下榜] {}{} (来源: {})\n",
                    i + 1,
                    title,
                    count_info,
                    platform.as_deref().unwrap_or("unknown")
                ));
            } else {
                let new_marker = if item.is_new.unwrap_or(false) { "新上榜 " } else { "" };
                output.push_str(&format!(
                    "{}. [{}#{}] {}{} (来源: {})\n",
                    i + 1,
                    new_marker,
                    rank,
                    title,
                    count_info,
                    platform.as_deref().unwrap_or("unknown")
                ));
            }
        }
        output
    }

    fn format_rss_content(&self, items: &[crate::model::RssItem]) -> String {
        if items.is_empty() {
            return "暂无RSS数据".to_string();
        }

        let mut output = String::new();
        for (i, item) in items.iter().enumerate() {
            output.push_str(&format!(
                "{}. {}{} (来源: {})\n   {}\n",
                i + 1,
                item.title,
                if item.title_changed {
                    " [标题已变更]"
                } else {
                    ""
                },
                item.feed_id
                    .as_deref()
                    .unwrap_or(&item.feed_name),
                item.summary
                    .as_deref()
                    .unwrap_or("无摘要")
            ));
        }
        output
    }
}

// ============================================================================
// AI 筛选器
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIFilterTag {
    pub id: i64,
    pub name: String,
    pub keywords: Vec<String>,
    pub priority: i32,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIFilterClassification {
    pub news_id: i64,
    pub tag_id: i64,
    pub score: f64,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ClassificationItem {
    Flat { id: i64, tag_id: i64, score: f64 },
    Nested { id: i64, tags: Vec<ClassificationNestedTag> },
}
#[derive(Deserialize, Debug)]
struct ClassificationNestedTag {
    tag_id: i64,
    score: f64,
}
#[derive(Deserialize)]
struct ClassificationResponse {
    results: Vec<ClassificationItem>,
}

pub struct AiFilter {
    client: AiClient,
    extract_prompt: (String, String),
    classify_prompt: (String, String),
    #[allow(dead_code)]
    update_tags_prompt: (String, String),
}

impl AiFilter {
    pub fn new(settings: &AiFilterSettings, global_ai: Option<&AiSettings>) -> Result<Self> {
        let ai_settings = AiSettings {
            provider: settings.provider.clone().or_else(|| global_ai.and_then(|g| g.provider.clone())),
            model: settings.model.clone().or_else(|| global_ai.and_then(|g| g.model.clone())),
            api_key: settings.api_key.clone().or_else(|| global_ai.and_then(|g| g.api_key.clone())),
            api_base: settings.api_base.clone().or_else(|| global_ai.and_then(|g| g.api_base.clone())),
            max_tokens: Some(global_ai.and_then(|g| g.max_tokens).unwrap_or(8192)),
            temperature: Some(0.3),
            timeout: Some(global_ai.and_then(|g| g.timeout).unwrap_or(120)),
        };

        let extract_prompt_path = settings.extract_prompt_file.as_deref().unwrap_or("ai_filter/extract_prompt.txt");
        let classify_prompt_path = settings.prompt_file.as_deref().unwrap_or("ai_filter/prompt.txt");
        let update_tags_prompt_path = settings.update_tags_prompt_file.as_deref().unwrap_or("ai_filter/update_tags_prompt.txt");

        let extract_prompt = load_prompt_template(extract_prompt_path)
            .unwrap_or_else(|_| {
                (
                    "你是一个专业的标签提取助手。请严格按JSON格式输出。".to_string(),
                    "从以下兴趣描述中提取结构化标签。以JSON格式返回：{\"tags\":[{\"name\":\"标签名\",\"keywords\":[\"关键词1\",\"关键词2\"],\"priority\":1}]}。\n\n兴趣描述：".to_string(),
                )
            });

        let classify_prompt = load_prompt_template(classify_prompt_path)
            .unwrap_or_else(|_| {
                (
                    "你是一个新闻分类助手。".to_string(),
                    "将以下新闻按标签进行分类。".to_string(),
                )
            });

        let update_tags_prompt =
            load_prompt_template(update_tags_prompt_path).unwrap_or_else(|_| {
                (
                    "你需要更新标签列表。".to_string(),
                    "对比旧标签和新兴趣描述，给出更新方案。".to_string(),
                )
            });

        Ok(AiFilter {
            client: AiClient::new(&ai_settings)?,
            extract_prompt,
            classify_prompt,
            update_tags_prompt,
        })
    }

    pub async fn extract_tags(&self, interests_content: &str) -> Result<Vec<AIFilterTag>> {
        let messages = vec![
            ChatMessage::system(&self.extract_prompt.0),
            ChatMessage::user(&format!(
                "{}\n\n{}",
                self.extract_prompt.1, interests_content
            )),
        ];

        #[derive(Deserialize)]
        struct ExtractResponse {
            tags: Vec<ExtractTag>,
        }
        #[derive(Deserialize)]
        struct ExtractTag {
            name: String,
            keywords: Vec<String>,
            priority: i32,
        }

        let response: ExtractResponse = self.client.chat_json(&messages).await?;
        Ok(response
            .tags
            .into_iter()
            .enumerate()
            .map(|(i, t)| AIFilterTag {
                id: (i + 1) as i64,
                name: t.name,
                keywords: t.keywords,
                priority: t.priority,
                parent_id: None,
            })
            .collect())
    }

    pub async fn classify_batch(
        &self,
        tags: &[AIFilterTag],
        titles: &[(i64, String)],
    ) -> Vec<AIFilterClassification> {
        if titles.is_empty() {
            return Vec::new();
        }

        let mut all_results = Vec::new();
        let chunk_size = 80usize;

        for chunk in titles.chunks(chunk_size) {
            let chunk_results = self.classify_one_chunk(tags, chunk, chunk_size).await;
            all_results.extend(chunk_results);
            if chunk.len() == chunk_size {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }

        all_results
    }

    async fn classify_one_chunk(
        &self,
        tags: &[AIFilterTag],
        chunk: &[(i64, String)],
        _initial_size: usize,
    ) -> Vec<AIFilterClassification> {
        let chunk_ids: Vec<i64> = chunk.iter().map(|(id, _)| *id).collect();
        let tags_text: String = tags
            .iter()
            .map(|t| format!("ID:{} - {} ({})", t.id, t.name, t.keywords.join(", ")))
            .collect::<Vec<_>>()
            .join("\n");
        let titles_text: String = chunk
            .iter()
            .map(|(id, title)| format!("ID:{} - {}", id, title))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "{}\n\n标签列表:\n{}\n\n新闻:\n{}\n\n为每条新闻匹配最合适的标签。输出格式:\nID|TAG_ID|SCORE\n每行一条，例如:\n1|3|0.95\n2|5|0.80\n未匹配的新闻不要输出。",
            self.classify_prompt.1, tags_text, titles_text
        );
        let messages = vec![
            ChatMessage::system(&self.classify_prompt.0),
            ChatMessage::user(&prompt),
        ];

        // 第 1-2 次尝试：AI 分类
        for attempt in 0..3u32 {
            if attempt > 0 {
                let wait = 2u64.pow(attempt);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }

            let response = match self.client.chat(&messages).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("AI 分类请求失败 (尝试 {}): {}", attempt, e);
                    continue;
                }
            };

            if response.trim().is_empty() {
                tracing::warn!("AI 返回空响应 (尝试 {})", attempt);
                continue;
            }

            // 尝试多层解析
            if let Some(results) = parse_classification_response(&response, &chunk_ids, tags) {
                return results;
            }
            tracing::warn!("AI 分类解析失败 (尝试 {}): {}...", attempt, &response[..response.len().min(100)]);
        }

        // 第 3 级兜底：关键词匹配，100% 可靠
        let fallback = keyword_fallback(chunk, tags);
        tracing::info!("AI 分类 → 关键词兜底匹配 {} 条", fallback.len());
        fallback
    }
}

fn parse_classification_response(
    response: &str,
    valid_ids: &[i64],
    tags: &[AIFilterTag],
) -> Option<Vec<AIFilterClassification>> {
    let id_set: std::collections::HashSet<i64> = valid_ids.iter().copied().collect();
    let tag_id_set: std::collections::HashSet<i64> = tags.iter().map(|t| t.id).collect();

    // 策略 A：JSON 解析
    let cleaned = extract_json_from_response(response);
    if let Ok(parsed) = serde_json::from_str::<ClassificationResponse>(&cleaned) {
        let results: Vec<_> = parsed.results.into_iter().filter_map(|item| {
            let (news_id, tag_id, score) = match item {
                ClassificationItem::Flat { id, tag_id, score } => (id, tag_id, score),
                ClassificationItem::Nested { id, tags } => {
                    let best = tags.into_iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))?;
                    (id, best.tag_id, best.score)
                }
            };
            if id_set.contains(&news_id) && tag_id_set.contains(&tag_id) {
                Some(AIFilterClassification { news_id, tag_id, score: score.clamp(0.0, 1.0) })
            } else {
                None
            }
        }).collect();
        if !results.is_empty() {
            tracing::info!("AI 分类 → JSON 解析成功，匹配 {} 条", results.len());
            return Some(results);
        }
    }

    // 策略 B：逐行 pipe 解析
    let line_results: Vec<_> = response.lines().filter_map(|line| {
        let parts: Vec<&str> = line.trim().split('|').collect();
        if parts.len() != 3 { return None; }
        let id: i64 = parts[0].trim().parse().ok()?;
        let tag_id: i64 = parts[1].trim().parse().ok()?;
        let score: f64 = parts[2].trim().parse().ok()?;
        if id_set.contains(&id) && tag_id_set.contains(&tag_id) {
            Some(AIFilterClassification { news_id: id, tag_id, score: score.clamp(0.0, 1.0) })
        } else {
            None
        }
    }).collect();
    if !line_results.is_empty() {
        tracing::info!("AI 分类 → 逐行 pipe 解析成功，匹配 {} 条", line_results.len());
        return Some(line_results);
    }

    None
}

fn keyword_fallback(
    chunk: &[(i64, String)],
    tags: &[AIFilterTag],
) -> Vec<AIFilterClassification> {
    chunk.iter().filter_map(|(id, title)| {
        let title_lower = title.to_lowercase();
        let mut best: Option<(i64, f64, usize)> = None;
        for tag in tags {
            let mut matches = 0usize;
            for kw in &tag.keywords {
                if title_lower.contains(&kw.to_lowercase()) {
                    matches += 1;
                }
            }
            if matches > 0 {
                let score = (matches as f64 / tag.keywords.len().max(1) as f64).clamp(0.0, 1.0);
                match &best {
                    None => best = Some((tag.id, score, matches)),
                    Some((_, prev_score, prev_matches)) => {
                        if score > *prev_score || (score == *prev_score && matches > *prev_matches) {
                            best = Some((tag.id, score, matches));
                        }
                    }
                }
            }
        }
        best.map(|(tag_id, score, _)| AIFilterClassification {
            news_id: *id,
            tag_id,
            score,
        })
    }).collect()
}

// ============================================================================
// AI 翻译器
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    pub original: String,
    pub translated: String,
    pub target_lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTranslationResult {
    pub results: Vec<TranslationResult>,
}

pub struct AiTranslator {
    client: AiClient,
    source_lang: String,
    target_lang: String,
    system_prompt: String,
    user_prompt_template: String,
}

impl AiTranslator {
    pub fn new(
        settings: &AiTranslationSettings,
        global_ai: Option<&AiSettings>,
        prompt_path: &str,
    ) -> Result<Self> {
        let ai_settings = AiSettings {
            provider: settings.provider.clone().or_else(|| global_ai.and_then(|g| g.provider.clone())),
            model: settings.model.clone().or_else(|| global_ai.and_then(|g| g.model.clone())),
            api_key: settings.api_key.clone().or_else(|| global_ai.and_then(|g| g.api_key.clone())),
            api_base: settings.api_base.clone().or_else(|| global_ai.and_then(|g| g.api_base.clone())),
            max_tokens: Some(global_ai.and_then(|g| g.max_tokens).unwrap_or(4096)),
            temperature: Some(0.3),
            timeout: Some(global_ai.and_then(|g| g.timeout).unwrap_or(120)),
        };

        let (system_prompt, user_prompt_template) =
            load_prompt_template(prompt_path).unwrap_or_else(|_| {
                (
                    "你是一个专业翻译助手。".to_string(),
                    "将以下文本从{source_lang}翻译成{target_lang}。".to_string(),
                )
            });

        Ok(AiTranslator {
            client: AiClient::new(&ai_settings)?,
            source_lang: settings
                .source_lang
                .clone()
                .unwrap_or_else(|| "zh".to_string()),
            target_lang: settings
                .target_lang
                .clone()
                .unwrap_or_else(|| "en".to_string()),
            system_prompt,
            user_prompt_template,
        })
    }

    pub async fn translate(&self, text: &str) -> Result<TranslationResult> {
        let mut user_prompt = self.user_prompt_template.clone();
        user_prompt = user_prompt.replace("{source_lang}", &self.source_lang);
        user_prompt = user_prompt.replace("{target_lang}", &self.target_lang);

        let messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(&format!("{}\n\n原文:\n{}", user_prompt, text)),
        ];

        let translated = self.client.chat(&messages).await?;

        Ok(TranslationResult {
            original: text.to_string(),
            translated,
            target_lang: self.target_lang.clone(),
        })
    }

    pub async fn translate_batch(&self, texts: &[String]) -> Result<BatchTranslationResult> {
        if texts.is_empty() {
            return Ok(BatchTranslationResult {
                results: Vec::new(),
            });
        }

        if texts.len() == 1 {
            let result = self.translate(&texts[0]).await?;
            return Ok(BatchTranslationResult {
                results: vec![result],
            });
        }

        let numbered: String = texts
            .iter()
            .enumerate()
            .map(|(i, text)| format!("[{}] {}", i + 1, text))
            .collect::<Vec<_>>()
            .join("\n\n");

        let mut user_prompt = self.user_prompt_template.clone();
        user_prompt = user_prompt.replace("{source_lang}", &self.source_lang);
        user_prompt = user_prompt.replace("{target_lang}", &self.target_lang);

        let messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(&format!(
                "{}\n\n请按照 [编号] 格式逐条翻译以下内容:\n\n{}",
                user_prompt, numbered
            )),
        ];

        let response = self.client.chat(&messages).await?;

        let mut results: Vec<Option<String>> = texts.iter().map(|_| None).collect();
        let mut current_num: Option<usize> = None;
        let mut current_text = String::new();

        for line in response.lines() {
            let trimmed = line.trim();
            if let Some(num) = parse_translation_number(trimmed) {
                if let Some(prev_num) = current_num {
                    if prev_num > 0 && prev_num <= results.len() {
                        results[prev_num - 1] =
                            Some(current_text.trim().to_string());
                    }
                }
                current_num = Some(num);
                current_text = String::new();
            } else if current_num.is_some() {
                if !current_text.is_empty() {
                    current_text.push('\n');
                }
                current_text.push_str(trimmed);
            }
        }

        if let Some(num) = current_num {
            if num > 0 && num <= results.len() && !current_text.trim().is_empty() {
                results[num - 1] = Some(current_text.trim().to_string());
            }
        }

        let final_results: Vec<TranslationResult> = texts
            .iter()
            .enumerate()
            .map(|(i, original)| TranslationResult {
                original: original.clone(),
                translated: results[i].clone().unwrap_or_else(|| original.clone()),
                target_lang: self.target_lang.clone(),
            })
            .collect();

        Ok(BatchTranslationResult {
            results: final_results,
        })
    }
}

fn parse_translation_number(text: &str) -> Option<usize> {
    let cleaned = text.trim_start_matches('[').trim_end_matches(']');
    if cleaned.len() <= 3 && cleaned.chars().all(|c| c.is_ascii_digit()) {
        cleaned.parse().ok()
    } else {
        None
    }
}

// ============================================================================
// AI 分析结果格式化器（多渠道路由）
// ============================================================================

impl AiAnalysisResult {
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        let sections = [
            (&self.core_trends, "🌐 核心热点与舆情态势"),
            (&self.sentiment_controversy, "🗣️ 舆论风向与争议"),
            (&self.signals, "📡 异动与弱信号"),
            (&self.rss_insights, "📰 RSS 深度洞察"),
            (&self.outlook_strategy, "📋 研判与策略建议"),
        ];
        for (value, title) in &sections {
            if value.is_null() { continue; }
            md.push_str(&format!("## {}

", title));
            md.push_str(&format_value(value, 0));
            md.push_str("

");
        }
        if let Some(obj) = self.standalone_summaries.as_object() {
            if !obj.is_empty() {
                md.push_str("\n---\n\n## 📌 专题分析\n\n");
                for (k, v) in obj {
                    let content = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
                    md.push_str(&format!("### {}\n\n{}\n\n", k, content));
                }
            }
        }
        md
    }
}

fn format_value(val: &serde_json::Value, indent: usize) -> String {
    match val {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() { String::new() } else { format!("{}
", trimmed) }
        }
        serde_json::Value::Array(arr) => {
            let mut s = String::new();
            for item in arr {
                match item {
                    serde_json::Value::String(text) => {
                        s.push_str(&format!("{}- {}
", "  ".repeat(indent), text));
                    }
                    serde_json::Value::Object(obj) => {
                        for (k, v) in obj {
                            match format_key_label(k) {
                                Some(label) => {
                                    s.push_str(&format!("{}**{}:** ", "  ".repeat(indent), label));
                                }
                                None => {
                                    s.push_str(&format!("{}", "  ".repeat(indent)));
                                }
                            }
                            match v {
                                serde_json::Value::String(t) => s.push_str(&format!("{}
", t)),
                                serde_json::Value::Array(a) => {
                                    s.push('\n');
                                    for elem in a {
                                        if let Some(t) = elem.as_str() {
                                            s.push_str(&format!("{}  - {}
", "  ".repeat(indent), t));
                                        }
                                    }
                                }
                                other => s.push_str(&format!("{}
", other)),
                            }
                        }
                    }
                    other => s.push_str(&format!("{}- {}
", "  ".repeat(indent), other)),
                }
            }
            s
        }
        serde_json::Value::Object(obj) => {
            let mut s = String::new();
            for (key, value) in obj {
                match value {
                    serde_json::Value::String(text) => {
                        match format_key_label(key) {
                            Some(label) => s.push_str(&format!("{}**{}:** {}\n", "  ".repeat(indent), label, text)),
                            None => s.push_str(&format!("{}{}\n", "  ".repeat(indent), text)),
                        }
                    }
                    serde_json::Value::Array(arr) => {
                        match format_key_label(key) {
                            Some(label) => s.push_str(&format!("{}**{}:**\n", "  ".repeat(indent), label)),
                            None => s.push_str(&format!("{}- {}", "  ".repeat(indent), "")),
                        }
                        for item in arr {
                            match item {
                                serde_json::Value::String(t) => {
                                    s.push_str(&format!("{}  - {}
", "  ".repeat(indent), t));
                                }
                                other => { s.push_str(&format_value(other, indent + 1)); }
                            }
                        }
                        s.push('\n');
                    }
                    serde_json::Value::Object(_) => {
                        match format_key_label(key) {
                            Some(label) => s.push_str(&format!("{}**{}:**
", "  ".repeat(indent), label)),
                            None => s.push_str(""),
                        }
                        s.push_str(&format_value(value, indent + 1));
                    }
                    other => {
                        match format_key_label(key) {
                            Some(label) => s.push_str(&format!("{}**{}:** {}
", "  ".repeat(indent), label, other)),
                            None => s.push_str(&format!("{}{}
", "  ".repeat(indent), other)),
                        }
                    }
                }
            }
            s
        }
        other => format!("{}
", other),
    }
}

pub(crate) fn format_key_label(key: &str) -> Option<String> {
    let label_map = [
        ("summary", "概要"), ("hot_topic_groups", "热点主题分组"), ("top_topics", "热点主题"),
        ("public_mood", "公众情绪"), ("public_sentiment", "公众情绪"),
        ("key_controversies", "核心争议"), ("controversies", "争议"),
        ("weak_signals", "弱信号"), ("potential_risks", "潜在风险"), ("risks", "风险"),
        ("tech_weekly_summary", "科技周报"), ("implications", "启示"),
        ("short_term_judgment", "短期研判"), ("mid_term_forecast", "中期展望"),
        ("recommended_actions", "建议行动"), ("actions", "行动建议"),
        ("hot_topics", "热门话题"), ("trends", "趋势"), ("insights", "洞察"),
        ("cross_platform_hot", "跨平台热点"), ("domestic", "国内热点"),
        ("international", "国际热点"), ("controversial_issues", "争议焦点"),
        ("public_reaction", "公众反应"), ("debate_points", "争论要点"),
        ("sentiment_distribution", "情绪分布"), ("positive", "正面"),
        ("negative", "负面"), ("neutral", "中性"), ("conclusion", "结论"),
        ("main_points", "要点"), ("analysis", "分析"), ("detail", "详情"),
        ("description", "描述"), ("overview", "概览"), ("recommendation", "建议"),
    ];
    for (en, zh) in &label_map {
        if key == *en { return Some(zh.to_string()); }
    }
    None
}

pub fn render_ai_analysis_markdown(result: &AiAnalysisResult) -> String {
    result.to_markdown()
}

pub fn render_ai_analysis_telegram(result: &AiAnalysisResult) -> String {
    crate::report::html_escape(&result.to_markdown())
}

pub fn render_ai_analysis_feishu(result: &AiAnalysisResult) -> String {
    result.to_markdown()
}

pub fn render_ai_analysis_dingtalk(result: &AiAnalysisResult) -> String {
    result.to_markdown()
}

pub fn render_ai_analysis_html(result: &AiAnalysisResult) -> String {
    let mut html = String::from(
        r#"<div style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; max-width: 800px; margin: 0 auto;">"#,
    );
    html.push_str(&markdown_to_html(&result.to_markdown()));
    html.push_str("</div>");
    html
}

pub fn render_ai_analysis_plain(result: &AiAnalysisResult) -> String {
    result.to_markdown()
}

// ============================================================================
// 辅助函数
// ============================================================================

fn markdown_to_html(text: &str) -> String {
    let text = text.replace("\n\n", "</p><p>");
    let text = text.replace('\n', "<br>");

    let text = AI_BOLD_RE.replace_all(&text, "<strong>$1</strong>");

    let text = format!("<p>{}</p>", text);
    text.replace("<p></p>", "")
}

// ============================================================================
// 便捷工厂方法
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_response() {
        let input = r#"```json
{"key": "value"}
```"#;
        assert_eq!(extract_json_from_response(input), r#"{"key": "value"}"#);

        let input_mixed = r#"这是分析结果：
{
  "core_trends": "内容"
}
以上为分析结果。"#;
        let result = extract_json_from_response(input_mixed);
        assert!(result.contains("\"core_trends\""));
    }

    #[test]
    fn test_render_markdown() {
        let result = AiAnalysisResult {
            core_trends: serde_json::json!({"summary": "测试核心内容"}),
            sentiment_controversy: serde_json::json!({"public_mood": "测试争议内容"}),
            signals: serde_json::json!({"weak_signals": ["测试信号"]}),
            rss_insights: serde_json::json!({"tech_weekly_summary": "测试RSS"}),
            outlook_strategy: serde_json::json!({"short_term_judgment": "测试策略"}),
            standalone_summaries: serde_json::json!({}),
        };

        let md = render_ai_analysis_markdown(&result);
        assert!(md.contains("🌐"));
        assert!(md.contains("核心热点"));
        assert!(md.contains("测试核心内容"));
    }

    #[test]
    fn test_chat_message() {
        let msg = ChatMessage::system("你是助手");
        assert_eq!(msg.role, "system");
        assert_eq!(msg.content, "你是助手");
    }
}
