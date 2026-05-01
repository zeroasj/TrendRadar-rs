use askama::Template;
use crate::ai::format_key_label;
use crate::report::{NewsDisplay, ReportData};

#[derive(Template)]
#[template(path = "report.html")]
pub struct ReportTemplate {
    pub title: String,
    pub mode_label: String,
    pub generation_time: String,
    pub stats: Vec<TemplateStatItem>,
    pub new_titles: Vec<TemplateSourceGroup>,
    pub failed_ids: Vec<String>,
    pub total_new_count: usize,
    pub total_items: usize,
    pub hot_news_count: usize,
    pub show_error: bool,
    pub show_stats: bool,
    pub show_new_section: bool,
    pub show_ai: bool,
    pub ai_blocks: Vec<TemplateAiBlock>,
    pub show_rss: bool,
    pub rss_groups: Vec<TemplateRssGroup>,
    pub rss_total: usize,
}

#[derive(Clone)]
pub struct TemplateAiBlock {
    pub title: String,
    pub content: String,
}

#[derive(Clone)]
pub struct TemplateStatItem {
    pub word: String,
    pub count: usize,
    pub index: usize,
    pub titles: Vec<TemplateTitleItem>,
}

#[derive(Clone)]
pub struct TemplateSourceGroup {
    pub source_name: String,
    pub titles: Vec<TemplateTitleItem>,
}

#[derive(Clone)]
pub struct TemplateTitleItem {
    pub number: usize,
    pub rank_text: String,
    pub rank_class: String,
    pub trend_arrow: String,
    pub time_display: String,
    pub title_count: usize,
    pub source_name: String,
    pub keyword: String,
    pub title: String,
    pub url: String,
    pub is_new: bool,
}

#[derive(Clone)]
pub struct TemplateRssGroup {
    pub name: String,
    pub count: usize,
    pub items: Vec<TemplateRssItem>,
}

#[derive(Clone)]
pub struct TemplateRssItem {
    pub title: String,
    pub time_display: String,
    pub source_name: String,
    pub url: String,
    pub is_new: bool,
}

fn convert_title(
    title_data: &NewsDisplay,
    keyword: &str,
    number: usize,
) -> TemplateTitleItem {
    let rank_text = if !title_data.ranks.is_empty() {
        let min_rank = title_data.ranks.iter().min().copied().unwrap_or(0);
        let max_rank = title_data.ranks.iter().max().copied().unwrap_or(0);
        if min_rank == max_rank {
            min_rank.to_string()
        } else {
            format!("{}-{}", min_rank, max_rank)
        }
    } else {
        String::new()
    };

    let rank_class = if !title_data.ranks.is_empty() {
        let min_rank = title_data.ranks.iter().min().copied().unwrap_or(99);
        if min_rank <= 3 {
            "top"
        } else if min_rank <= title_data.rank_threshold {
            "high"
        } else {
            ""
        }
    } else {
        ""
    }
    .to_string();

    let trend_arrow = if title_data.ranks.len() >= 2 {
        let prev_rank = title_data.ranks[title_data.ranks.len() - 2];
        let curr_rank = title_data.ranks[title_data.ranks.len() - 1];
        if curr_rank < prev_rank {
            "🔺"
        } else if curr_rank > prev_rank {
            "🔻"
        } else {
            "➖"
        }
    } else {
        ""
    };

    let simplified_time = title_data
        .time_display
        .replace(" ~ ", "~")
        .replace('[', "")
        .replace(']', "");

    TemplateTitleItem {
        title: title_data.title.clone(),
        source_name: title_data.source_name.clone(),
        time_display: simplified_time,
        keyword: keyword.to_string(),
        rank_text,
        rank_class,
        trend_arrow: trend_arrow.to_string(),
        url: if !title_data.mobile_url.is_empty() {
            title_data.mobile_url.clone()
        } else {
            title_data.url.clone()
        },
        is_new: title_data.is_new,
        title_count: title_data.count,
        number,
    }
}

impl ReportTemplate {
    pub fn from_report_data(data: &ReportData, mode_label: &str) -> Self {
        let stats: Vec<TemplateStatItem> = data
            .stats
            .iter()
            .enumerate()
            .map(|(i, stat)| {
                let titles: Vec<TemplateTitleItem> = stat
                    .titles
                    .iter()
                    .enumerate()
                    .map(|(j, title)| convert_title(title, &stat.word, j + 1))
                    .collect();
                TemplateStatItem {
                    word: stat.word.clone(),
                    count: stat.count,
                    index: i + 1,
                    titles,
                }
            })
            .collect();

        let new_titles: Vec<TemplateSourceGroup> = data
            .new_titles
            .iter()
            .map(|source| {
                let titles: Vec<TemplateTitleItem> = source
                    .titles
                    .iter()
                    .enumerate()
                    .map(|(j, title)| convert_title(title, "", j + 1))
                    .collect();
                TemplateSourceGroup {
                    source_name: source.source_name.clone(),
                    titles,
                }
            })
            .collect();

        let hot_news_count = data.stats.iter().map(|s| s.titles.len()).sum();

        let ai_blocks: Vec<TemplateAiBlock> = if let Some(ref ai) = data.ai_analysis {
            let mut blocks = Vec::new();
            let sections = [
                (&ai.core_trends, "🌐 核心热点与舆情态势"),
                (&ai.sentiment_controversy, "🗣️ 舆论风向与争议"),
                (&ai.signals, "📡 异动与弱信号"),
                (&ai.rss_insights, "📰 RSS 深度洞察"),
                (&ai.outlook_strategy, "📋 研判与策略建议"),
            ];
            for (value, title) in &sections {
                if value.is_null() { continue; }
                let content = format_value(value, 0);
                if content.trim().is_empty() { continue; }
                blocks.push(TemplateAiBlock {
                    title: title.to_string(),
                    content,
                });
            }
            if let Some(obj) = ai.standalone_summaries.as_object() {
                for (k, v) in obj {
                    let content = format_value(v, 0);
                    if content.trim().is_empty() { continue; }
                    blocks.push(TemplateAiBlock {
                        title: format!("📌 {}", k),
                        content,
                    });
                }
            }
            blocks
        } else {
            vec![]
        };

        let rss_groups: Vec<TemplateRssGroup> = data.rss_groups.iter().map(|g| {
            TemplateRssGroup {
                name: g.name.clone(),
                count: g.count,
                items: g.items.iter().map(|i| TemplateRssItem {
                    title: i.title.clone(),
                    time_display: i.time_display.clone(),
                    source_name: i.source_name.clone(),
                    url: i.url.clone(),
                    is_new: i.is_new,
                }).collect(),
            }
        }).collect();
        let rss_total: usize = data.rss_total;

        ReportTemplate {
            title: data.title.clone(),
            mode_label: mode_label.to_string(),
            generation_time: data.generation_time.clone(),
            stats: stats.clone(),
            new_titles,
            failed_ids: data.failed_ids.clone(),
            total_new_count: data.total_new_count,
            total_items: data.total_items,
            hot_news_count,
            show_error: !data.failed_ids.is_empty(),
            show_stats: !stats.is_empty(),
            show_new_section: !data.new_titles.is_empty(),
            show_ai: !ai_blocks.is_empty(),
            ai_blocks,
            show_rss: !rss_groups.is_empty(),
            rss_groups,
            rss_total,
        }
    }
}

fn format_value(value: &serde_json::Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        serde_json::Value::String(s) => format!("{}{}", prefix, s),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let content = format_value(v, indent + 1);
                    if content.trim().is_empty() {
                        String::new()
                    } else {
                        format!("{}{}. {}", prefix, i + 1, content.trim())
                    }
                })
                .filter(|s| !s.is_empty())
                .collect();
            items.join("\n")
        }
        serde_json::Value::Object(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let content = format_value(v, indent + 1);
                    if content.trim().is_empty() {
                        String::new()
                    } else {
                        match format_key_label(k) {
                            Some(label) => format!("{}**{}**: {}", prefix, label, content.trim()),
                            None => format!("{}{}", prefix, content.trim()),
                        }
                    }
                })
                .filter(|s| !s.is_empty())
                .collect();
            items.join("\n")
        }
        serde_json::Value::Number(n) => format!("{}{}", prefix, n),
        serde_json::Value::Bool(b) => format!("{}{}", prefix, b),
        serde_json::Value::Null => String::new(),
    }
}
