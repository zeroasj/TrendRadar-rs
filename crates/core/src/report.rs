use crate::ai::AiAnalysisResult;
use crate::config::{DisplaySettings, RssFeed};
use regex::Regex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::LazyLock;

static HTML_BOLD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").unwrap());
static HTML_ITALIC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*(.+?)\*").unwrap());
static HTML_LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());

// ============================================================================
// 报告数据模型（用于模板渲染）
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct ReportData {
    pub title: String,
    pub mode: String,
    pub generation_time: String,
    pub stats: Vec<StatItem>,
    pub new_titles: Vec<SourceTitles>,
    pub failed_ids: Vec<String>,
    pub total_new_count: usize,
    pub total_items: usize,
    pub update_info: Option<UpdateInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_analysis: Option<AiAnalysisResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatItem {
    pub word: String,
    pub count: usize,
    pub percentage: f64,
    pub titles: Vec<NewsDisplay>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NewsDisplay {
    pub title: String,
    pub source_name: String,
    pub time_display: String,
    pub count: usize,
    pub ranks: Vec<i32>,
    pub rank_threshold: i32,
    pub url: String,
    pub mobile_url: String,
    pub is_new: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceTitles {
    pub source_id: String,
    pub source_name: String,
    pub titles: Vec<NewsDisplay>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub last_crawl_time: String,
    pub next_crawl_time: String,
    pub elapsed: String,
}

// ============================================================================
// Markdown 报告生成
// ============================================================================

pub fn generate_markdown_report(
    data: &ReportData,
    display: &DisplaySettings,
) -> String {
    let mut md = String::new();
    let mode_label = data.mode.clone();

    if display.show_title_section {
        md.push_str(&format!(
            "# 📊 TrendRadar {}报告\n\n**生成时间**: {}\n\n",
            mode_label, data.generation_time
        ));
    }

    if display.show_total_statistics {
        md.push_str(&format!(
            "**📊 统计概览**\n\n| 指标 | 数值 |\n|------|------|\n"
        ));
        md.push_str(&format!("| 📰 总新闻数 | {} |\n", data.total_items));
        md.push_str(&format!("| 🆕 新增热点 | {} |\n", data.total_new_count));
        md.push_str(&format!(
            "| 💥 热点关键词 | {} |\n\n",
            data.stats.len()
        ));
    }

    if display.show_frequency_analysis && !data.stats.is_empty() {
        md.push_str("## 🔥 关键词频率分析\n\n");
        for (i, stat) in data.stats.iter().enumerate() {
            md.push_str(&format!(
                "### {}. {} (出现 {} 次，占比 {:.1}%)\n\n",
                i + 1,
                stat.word,
                stat.count,
                stat.percentage
            ));

            for title in &stat.titles {
                let change_indicator = if title.is_new {
                    "🆕 "
                } else {
                    ""
                };
                let trend = if title.ranks.len() >= 2 {
                    let prev = title.ranks[title.ranks.len() - 2];
                    let curr = title.ranks[title.ranks.len() - 1];
                    if curr < prev {
                        " 🔺"
                    } else if curr > prev {
                        " 🔻"
                    } else {
                        " ➖"
                    }
                } else {
                    ""
                };
                let rank_display = if !title.ranks.is_empty() {
                    let min_r = title.ranks.iter().min().copied().unwrap_or(0);
                    let max_r = title.ranks.iter().max().copied().unwrap_or(0);
                    if min_r == max_r {
                        format!("[#{}]", min_r)
                    } else {
                        format!("[#{}-{}]", min_r, max_r)
                    }
                } else {
                    String::new()
                };
                md.push_str(&format!(
                    "- {}{}{} {} — {}{}\n",
                    change_indicator,
                    rank_display,
                    trend,
                    title.title,
                    title.source_name,
                    if !title.time_display.is_empty() {
                        format!(" ({})", title.time_display)
                    } else {
                        String::new()
                    }
                ));
            }
            md.push('\n');
        }
    }

    if display.show_new_section && !data.new_titles.is_empty() {
        md.push_str(&format!(
            "## 🆕 新增热点 (共 {} 条)\n\n",
            data.total_new_count
        ));
        for source in &data.new_titles {
            md.push_str(&format!("### {}\n\n", source.source_name));
            for title in &source.titles {
                let url_display = if !title.url.is_empty() {
                    format!("[链接]({})", title.url)
                } else {
                    String::new()
                };
                md.push_str(&format!("- {} {}\n", title.title, url_display));
            }
            md.push('\n');
        }
    }

    if display.show_failed_sources && !data.failed_ids.is_empty() {
        md.push_str(&format!(
            "## ⚠️ 失败的数据源 ({})\n\n",
            data.failed_ids.len()
        ));
        for id in &data.failed_ids {
            md.push_str(&format!("- {}\n", id));
        }
        md.push('\n');
    }

    md
}

// ============================================================================
// RSS 报告生成
// ============================================================================

pub fn generate_rss_report(
    data: &ReportData,
    rss_feed: Option<&RssFeed>,
) -> String {
    let feed_title = rss_feed
        .and_then(|f| f.title.as_deref())
        .unwrap_or("TrendRadar");
    let feed_desc = rss_feed
        .and_then(|f| f.description.as_deref())
        .unwrap_or("多平台热点聚合报告");

    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push_str(&format!(
        r#"<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom"><channel><title>{}</title><link>https://github.com/sansan0/TrendRadar</link><description>{}</description><lastBuildDate>{}</lastBuildDate><generator>TrendRadar-rs</generator>"#,
        xml_escape(feed_title),
        xml_escape(feed_desc),
        data.generation_time
    ));

    for stat in &data.stats {
        for title in &stat.titles {
            let link = if !title.url.is_empty() {
                title.url.clone()
            } else {
                format!("https://trendradar.local/item/{}", stat.word)
            };
            let desc = format!(
                "来源: {} | 关键词: {} | 出现次数: {}",
                title.source_name, stat.word, stat.count
            );
            xml.push_str(&format!(
                r#"<item><title>{}</title><link>{}</link><description>{}</description><pubDate>{}</pubDate><source url="{}">{}</source></item>"#,
                xml_escape(&title.title),
                xml_escape(&link),
                xml_escape(&desc),
                data.generation_time,
                xml_escape(&link),
                xml_escape(&title.source_name)
            ));
        }
    }

    xml.push_str("</channel></rss>");
    xml
}

// ============================================================================
// 通知内容生成（精简版 Markdown for 推送渠道）
// ============================================================================

pub fn generate_notification_content(
    data: &ReportData,
    display: &DisplaySettings,
    channel: &str,
) -> String {
    let mut content = String::new();
    let mode_label = data.mode.clone();

    let channel_specific_title = match channel {
        "feishu" => format!(
            "**📊 TrendRadar {}报告**\n生成时间：{}\n\n",
            mode_label, data.generation_time
        ),
        "dingtalk" => format!(
            "## 📊 TrendRadar {}报告\n生成时间：{}\n\n",
            mode_label, data.generation_time
        ),
        "wecom" | "slack" => format!(
            "📊 **TrendRadar {}报告**\n生成时间：{}\n\n",
            mode_label, data.generation_time
        ),
        "telegram" => format!(
            "<b>📊 TrendRadar {}报告</b>\n生成时间：{}\n\n",
            mode_label, data.generation_time
        ),
        "bark" | "ntfy" => format!(
            "📊 TrendRadar {}报告\n\n",
            mode_label
        ),
        _ => format!(
            "📊 TrendRadar {}报告\n生成时间：{}\n\n",
            mode_label, data.generation_time
        ),
    };
    content.push_str(&channel_specific_title);

    if display.show_total_statistics {
        content.push_str(&format!(
            "📊 总新闻数: {} | 🆕 新增热点: {} | 💥 关键词: {}\n\n",
            data.total_items, data.total_new_count, data.stats.len()
        ));
    }

    if let Some(ref ai) = data.ai_analysis {
        content.push_str(&ai.to_markdown());
        content.push_str("\n---\n\n");
    }

    if display.show_frequency_analysis && !data.stats.is_empty() {
        let section_title = if data.stats.iter().any(|s| s.count > 1 && s.titles.iter().any(|t| t.source_name != s.word)) {
            "🔥 **关键词热度分析**\n\n"
        } else {
            "📱 **各平台热点**\n\n"
        };
        content.push_str(section_title);
        for (i, stat) in data.stats.iter().enumerate() {
            if i >= display.max_keywords_display.unwrap_or(10) {
                break;
            }
            content.push_str(&format!(
                "**{}. {}** ({}次, {:.1}%)\n",
                i + 1,
                stat.word,
                stat.count,
                stat.percentage
            ));

            for (j, title) in stat.titles.iter().enumerate() {
                if j >= display.max_titles_per_keyword.unwrap_or(5) {
                    break;
                }
                let new_marker = if title.is_new { "🆕 " } else { "" };
                content.push_str(&format!("  - {}{}\n", new_marker, title.title));
            }
            content.push('\n');
        }
    }

    content
}

// ============================================================================
// 批次拆分（按字节大小拆分）
// ============================================================================

pub fn split_content_by_bytes(content: &str, max_bytes: usize) -> Vec<String> {
    let bytes = content.as_bytes();
    let total = bytes.len();
    if total <= max_bytes {
        return vec![content.to_string()];
    }

    let mut batches: Vec<String> = Vec::new();
    let mut start = 0;

    while start < total {
        let mut end = (start + max_bytes).min(total);
        if end < total {
            while end > start && bytes[end] & 0xC0 == 0x80 {
                end -= 1;
            }
            while end > start && bytes[end - 1] != b'\n' {
                end -= 1;
            }
            if end == start {
                end = (start + max_bytes).min(total);
                while end < total && bytes[end] & 0xC0 == 0x80 {
                    end += 1;
                }
            }
        }
        let chunk = String::from_utf8_lossy(&bytes[start..end]).to_string();
        batches.push(chunk);
        start = end;
    }

    batches
}

// ============================================================================
// HTML 报告文件操作
// ============================================================================

pub struct ReportWriter {
    output_dir: PathBuf,
}

impl ReportWriter {
    pub fn new(output_dir: &str) -> Self {
        let dir = PathBuf::from(output_dir);
        std::fs::create_dir_all(&dir).ok();
        ReportWriter { output_dir: dir }
    }

    pub fn write_snapshot(&self, date_folder: &str, time_filename: &str, html: &str) -> String {
        let snapshot_dir = self.output_dir.join("html").join(date_folder);
        if let Err(e) = std::fs::create_dir_all(&snapshot_dir) {
            tracing::warn!("创建快照目录失败: {}", e);
        }
        let path = snapshot_dir.join(format!("{}.html", time_filename));
        if let Err(e) = std::fs::write(&path, html) {
            tracing::warn!("写入快照文件失败: {}", e);
        }
        path.to_string_lossy().to_string()
    }

    pub fn write_latest(&self, mode: &str, html: &str) -> String {
        let latest_dir = self.output_dir.join("html").join("latest");
        if let Err(e) = std::fs::create_dir_all(&latest_dir) {
            tracing::warn!("创建latest目录失败: {}", e);
        }
        let path = latest_dir.join(format!("{}.html", mode));
        if let Err(e) = std::fs::write(&path, html) {
            tracing::warn!("写入latest文件失败: {}", e);
        }
        path.to_string_lossy().to_string()
    }

    pub fn write_index(&self, html: &str) {
        let output_index = self.output_dir.join("index.html");
        if let Err(e) = std::fs::write(&output_index, html) {
            tracing::warn!("写入index文件失败: {}", e);
        }
        let root_index = PathBuf::from("index.html");
        if let Err(e) = std::fs::write(&root_index, html) {
            tracing::warn!("写入根index文件失败: {}", e);
        }
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn get_batch_size_for_channel(channel: &str) -> usize {
    match channel {
        "feishu" => 29000,
        "dingtalk" => 20000,
        "wecom" => 4000,
        "telegram" => 4000,
        "bark" => 3600,
        "ntfy" => 3800,
        "slack" => 4000,
        "email" => 500_000,
        "webhook" => 100_000,
        _ => 3000,
    }
}

pub fn markdown_to_simple_html(md: &str) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0"><style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;max-width:800px;margin:0 auto;padding:20px;line-height:1.6;color:#333}h1,h2,h3{color:#1a1a1a}hr{border:none;border-top:1px solid #eee;margin:20px 0}strong{color:#000}a{color:#0066cc}</style></head><body>"#,
    );

    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("### ") {
            html.push_str(&format!("<h3>{}</h3>", html_escape(&trimmed[4..])));
        } else if trimmed.starts_with("## ") {
            html.push_str(&format!("<h2>{}</h2>", html_escape(&trimmed[3..])));
        } else if trimmed.starts_with("# ") {
            html.push_str(&format!("<h1>{}</h1>", html_escape(&trimmed[2..])));
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            html.push_str("<hr>");
        } else if trimmed.is_empty() {
            html.push_str("<br>");
        } else {
            let text = HTML_BOLD_RE.replace_all(trimmed, "<strong>$1</strong>");
            let text = HTML_ITALIC_RE.replace_all(&text, "<em>$1</em>");
            let text = HTML_LINK_RE.replace_all(&text, r#"<a href="$2">$1</a>"#);
            html.push_str(&format!("<p>{}</p>", text));
        }
    }

    html.push_str("</body></html>");
    html
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_report() -> ReportData {
        ReportData {
            title: "Test Report".to_string(),
            mode: "daily".to_string(),
            generation_time: "2024-01-01 12:00:00".to_string(),
            stats: vec![
                StatItem {
                    word: "AI".to_string(),
                    count: 10,
                    percentage: 50.0,
                    titles: vec![NewsDisplay {
                        title: "AI突破新闻".to_string(),
                        source_name: "知乎".to_string(),
                        time_display: "12:00".to_string(),
                        count: 1,
                        ranks: vec![1],
                        rank_threshold: 3,
                        url: "https://example.com".to_string(),
                        mobile_url: String::new(),
                        is_new: true,
                    }],
                },
                StatItem {
                    word: "芯片".to_string(),
                    count: 5,
                    percentage: 25.0,
                    titles: vec![NewsDisplay {
                        title: "芯片新突破".to_string(),
                        source_name: "微博".to_string(),
                        time_display: "11:00".to_string(),
                        count: 1,
                        ranks: vec![2],
                        rank_threshold: 3,
                        url: String::new(),
                        mobile_url: String::new(),
                        is_new: false,
                    }],
                },
            ],
            new_titles: vec![],
            failed_ids: vec!["test_platform".to_string()],
            total_new_count: 1,
            total_items: 15,
            update_info: None,
            ai_analysis: None,
        }
    }

    #[test]
    fn test_generate_markdown_report() {
        let data = make_test_report();
        let display = DisplaySettings::default();
        let md = generate_markdown_report(&data, &display);
        assert!(md.contains("AI突破新闻"));
        assert!(md.contains("AI"));
        assert!(md.contains("芯片"));
    }

    #[test]
    fn test_split_content_by_bytes() {
        let content = "这是一段测试文本，用于测试分批功能。\n第二行内容。\n第三行。";
        let batches = split_content_by_bytes(content, 20);
        assert!(!batches.is_empty());
        let recombined: String = batches.concat();
        assert_eq!(recombined, content);
    }

    #[test]
    fn test_generate_rss_report() {
        let data = make_test_report();
        let rss = generate_rss_report(&data, None);
        assert!(rss.contains("<?xml"));
        assert!(rss.contains("<rss"));
        assert!(rss.contains("<item>"));
    }
}
