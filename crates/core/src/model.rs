use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: Option<i64>,
    pub url: Option<String>,
    pub title: String,
    pub platform: String,
    pub platform_name: Option<String>,
    pub rank: Option<i32>,
    pub hot_score: Option<f64>,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub publish_time: Option<DateTime<Utc>>,
    pub crawl_time: DateTime<Utc>,
    pub category: Option<String>,
    pub keywords: Vec<String>,
    pub is_new: Option<bool>,
    pub rank_change: Option<i32>,
    pub title_changed: Option<bool>,
    pub appearance_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsData {
    pub items: Vec<NewsItem>,
    pub total: usize,
    pub crawl_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssItem {
    pub title: String,
    pub link: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub publish_time: Option<DateTime<Utc>>,
    pub crawl_time: DateTime<Utc>,
    pub feed_name: String,
    pub feed_id: Option<String>,
    pub guid: Option<String>,
    pub keywords: Vec<String>,
    pub title_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssData {
    pub items: Vec<RssItem>,
    pub total: usize,
    pub crawl_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReportMode {
    Incremental,
    Current,
    Daily,
}

impl std::str::FromStr for ReportMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "incremental" => Ok(ReportMode::Incremental),
            "current" => Ok(ReportMode::Current),
            "daily" => Ok(ReportMode::Daily),
            _ => Err(format!("Unknown report mode: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NotificationChannel {
    Feishu,
    Dingtalk,
    Wecom,
    Telegram,
    Email,
    Bark,
    Ntfy,
    Slack,
    Webhook,
}

impl std::str::FromStr for NotificationChannel {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "feishu" => Ok(NotificationChannel::Feishu),
            "dingtalk" => Ok(NotificationChannel::Dingtalk),
            "wecom" => Ok(NotificationChannel::Wecom),
            "telegram" => Ok(NotificationChannel::Telegram),
            "email" => Ok(NotificationChannel::Email),
            "bark" => Ok(NotificationChannel::Bark),
            "ntfy" => Ok(NotificationChannel::Ntfy),
            "slack" => Ok(NotificationChannel::Slack),
            "webhook" => Ok(NotificationChannel::Webhook),
            _ => Err(format!("Unknown notification channel: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub enabled: bool,
}
