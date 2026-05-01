use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_one_or_many<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    match value {
        serde_yaml::Value::Null => Ok(vec![]),
        serde_yaml::Value::Sequence(seq) => seq
            .into_iter()
            .map(|v| T::deserialize(v).map_err(|e| serde::de::Error::custom(e.to_string())))
            .collect(),
        other => {
            let single = T::deserialize(other)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
            Ok(vec![single])
        }
    }
}

fn deserialize_empty_as_none<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    match opt {
        None | Some(serde_yaml::Value::Null) => Ok(None),
        Some(serde_yaml::Value::String(s)) if s.is_empty() => Ok(None),
        Some(v) => T::deserialize(v)
            .map(Some)
            .map_err(|e| serde::de::Error::custom(e.to_string())),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub app: AppSettings,
    #[serde(default)]
    pub schedule: Option<ScheduleSettings>,
    #[serde(default)]
    pub platforms: PlatformSettings,
    #[serde(default)]
    pub rss: Option<RssSettings>,
    #[serde(default)]
    pub report: Option<ReportSettings>,
    #[serde(default)]
    pub filter: Option<FilterSettings>,
    #[serde(default)]
    pub ai_filter: Option<AiFilterSettings>,
    #[serde(default)]
    pub display: Option<DisplaySettings>,
    #[serde(default)]
    pub notification: Option<NotificationSettings>,
    #[serde(default)]
    pub storage: Option<StorageSettings>,
    #[serde(default)]
    pub ai: Option<AiSettings>,
    #[serde(default)]
    pub ai_analysis: Option<AiAnalysisSettings>,
    #[serde(default)]
    pub ai_translation: Option<AiTranslationSettings>,
    #[serde(default)]
    pub advanced: Option<AdvancedSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub show_schedule: Option<bool>,
    #[serde(default)]
    pub max_workers: Option<usize>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleSettings {
    pub enabled: Option<bool>,
    pub preset: Option<String>,
    #[serde(default)]
    pub periods: Vec<SchedulePeriod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulePeriod {
    pub id: String,
    pub name: Option<String>,
    pub enabled: bool,
    pub cron: Option<String>,
    #[serde(default)]
    pub run_days: Option<Vec<i32>>,
    #[serde(default)]
    pub run_hours: Option<Vec<u32>>,
    pub run_minutes: Option<Vec<u32>>,
    pub collect: Option<bool>,
    pub analyze: Option<bool>,
    pub push: Option<bool>,
    pub report_mode: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sources: Vec<PlatformSource>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub max_items: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformSource {
    pub id: String,
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RssSettings {
    pub enabled: Option<bool>,
    #[serde(default)]
    pub feeds: Vec<RssFeed>,
    #[serde(default)]
    pub freshness_filter: Option<RssFreshnessFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssFreshnessFilter {
    pub enabled: bool,
    pub max_age_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssFeed {
    pub name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub max_age_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReportSettings {
    pub enabled: Option<bool>,
    pub mode: Option<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilterSettings {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub exclude_keywords: Vec<String>,
    #[serde(default)]
    pub min_title_length: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiFilterSettings {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    #[serde(default)]
    pub batch_size: Option<usize>,
    #[serde(default)]
    pub batch_interval: Option<u64>,
    #[serde(default)]
    pub min_score: Option<f64>,
    #[serde(default)]
    pub reclassify_threshold: Option<f64>,
    #[serde(default)]
    pub interests_file: Option<String>,
    #[serde(default)]
    pub prompt_file: Option<String>,
    #[serde(default)]
    pub extract_prompt_file: Option<String>,
    #[serde(default)]
    pub update_tags_prompt_file: Option<String>,
    #[serde(default)]
    pub priority_sort_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    pub max_items: Option<usize>,
    pub show_rank: Option<bool>,
    pub show_title_section: bool,
    pub show_total_statistics: bool,
    pub show_frequency_analysis: bool,
    pub show_new_section: bool,
    pub show_failed_sources: bool,
    pub show_rss_section: bool,
    pub show_ai_section: bool,
    pub max_keywords_display: Option<usize>,
    pub max_titles_per_keyword: Option<usize>,
    #[serde(default)]
    pub region_order: Vec<String>,
    #[serde(default)]
    pub standalone: StandaloneSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StandaloneSettings {
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub rss_feeds: Vec<String>,
    #[serde(default)]
    pub max_items: Option<usize>,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            max_items: None,
            show_rank: None,
            show_title_section: true,
            show_total_statistics: true,
            show_frequency_analysis: true,
            show_new_section: true,
            show_failed_sources: true,
            show_rss_section: true,
            show_ai_section: true,
            max_keywords_display: None,
            max_titles_per_keyword: None,
            region_order: vec![
                "hotlist".to_string(),
                "rss".to_string(),
                "new_items".to_string(),
                "ai_analysis".to_string(),
            ],
            standalone: StandaloneSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationSettings {
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channels: NotificationChannels,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationChannels {
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub feishu: Vec<FeishuConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub dingtalk: Vec<DingtalkConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub wework: Vec<WecomConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub telegram: Vec<TelegramConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub email: Vec<EmailConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub bark: Vec<BarkConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub ntfy: Vec<NtfyConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub slack: Vec<SlackConfig>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    pub generic_webhook: Vec<WebhookConfig>,
}

impl NotificationChannels {
    pub fn effective_channels(&self) -> Vec<(&str, bool)> {
        let mut result = Vec::new();
        result.push(("feishu", self.feishu.iter().any(|c| !c.webhook_url.is_empty())));
        result.push(("dingtalk", self.dingtalk.iter().any(|c| !c.webhook_url.is_empty())));
        result.push(("wecom", self.wework.iter().any(|c| !c.webhook_url.is_empty())));
        result.push(("telegram", self.telegram.iter().any(|c| !c.bot_token.is_empty() && !c.chat_id.is_empty())));
        result.push(("email", self.email.iter().any(|c| !c.from.is_empty() && !c.password.is_empty() && !c.to.is_empty())));
        result.push(("bark", self.bark.iter().any(|c| !c.url.is_empty())));
        result.push(("ntfy", self.ntfy.iter().any(|c| !c.server_url.is_empty() && !c.topic.is_empty())));
        result.push(("slack", self.slack.iter().any(|c| !c.webhook_url.is_empty())));
        result.push(("webhook", self.generic_webhook.iter().any(|c| !c.webhook_url.is_empty())));
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    pub webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingtalkConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WecomConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub msg_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    #[serde(default)]
    pub smtp_server: Option<String>,
    #[serde(default, deserialize_with = "deserialize_empty_as_none")]
    pub smtp_port: Option<u16>,
    pub from: String,
    pub password: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarkConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtfyConfig {
    pub server_url: String,
    pub topic: String,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub payload_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageSettings {
    #[serde(default = "default_storage_backend")]
    pub backend: String,
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub s3_bucket: Option<String>,
    #[serde(default)]
    pub s3_region: Option<String>,
    #[serde(default)]
    pub s3_access_key: Option<String>,
    #[serde(default)]
    pub s3_secret_key: Option<String>,
    #[serde(default)]
    pub s3_endpoint: Option<String>,
}

fn default_storage_backend() -> String {
    "local".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiSettings {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiAnalysisSettings {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    #[serde(default)]
    pub prompt_file: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub max_news_for_analysis: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiTranslationSettings {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    #[serde(default)]
    pub source_lang: Option<String>,
    #[serde(default)]
    pub target_lang: Option<String>,
    #[serde(default)]
    pub prompt_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdvancedSettings {
    pub request_delay: Option<u64>,
    pub request_timeout: Option<u64>,
    pub max_retries: Option<u32>,
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub data_retention_days: Option<i64>,
}

impl AppConfig {
    pub fn load(path: &str) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::error::TrendRadarError::Config(format!("Failed to read config: {}", e)))?;
        let mut config: Self = serde_yaml::from_str(&content)
            .map_err(|e| crate::error::TrendRadarError::Config(format!("Failed to parse config: {}", e)))?;
        config.apply_env_overrides();
        Ok(config)
    }

    fn apply_env_overrides(&mut self) {
        let common_api_key = std::env::var("AI_API_KEY").ok().filter(|s| !s.is_empty());

        if let Some(ref mut ai) = self.ai {
            try_env_override("AI_API_KEY", &mut ai.api_key);
        }
        if let Some(ref mut ai_analysis) = self.ai_analysis {
            try_env_override("AI_ANALYSIS_API_KEY", &mut ai_analysis.api_key);
            if ai_analysis.api_key.is_none() {
                if let Some(ref key) = common_api_key {
                    ai_analysis.api_key = Some(key.clone());
                }
            }
        }
        if let Some(ref mut ai_filter) = self.ai_filter {
            try_env_override("AI_FILTER_API_KEY", &mut ai_filter.api_key);
            if ai_filter.api_key.is_none() {
                if let Some(ref key) = common_api_key {
                    ai_filter.api_key = Some(key.clone());
                }
            }
        }
        if let Some(ref mut ai_translation) = self.ai_translation {
            try_env_override("AI_TRANSLATION_API_KEY", &mut ai_translation.api_key);
            if ai_translation.api_key.is_none() {
                if let Some(ref key) = common_api_key {
                    ai_translation.api_key = Some(key.clone());
                }
            }
        }

        if let Ok(pw) = std::env::var("SMTP_PASSWORD") {
            if !pw.is_empty() {
                if let Some(ref mut notification) = self.notification {
                    for email in &mut notification.channels.email {
                        if email.password.is_empty() {
                            email.password = pw.clone();
                        }
                    }
                }
            }
        }
    }
}

fn try_env_override(env_key: &str, target: &mut Option<String>) {
    if let Ok(val) = std::env::var(env_key) {
        if !val.is_empty() {
            *target = Some(val);
        }
    }
}
