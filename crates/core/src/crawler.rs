use crate::config::AppConfig;
use crate::error::{Result, TrendRadarError};
use crate::model::{NewsItem, RssItem, RssData};
use chrono::Utc;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, CONNECTION, USER_AGENT};
use std::collections::HashMap;
use std::time::Duration;

const NEWSNOW_API_URL: &str = "https://newsnow.busiyi.world/api/s";

const PLATFORM_IDS: &[(&str, &str)] = &[
    ("weibo", "微博"),
    ("zhihu", "知乎"),
    ("baidu", "百度"),
    ("toutiao", "头条"),
    ("bilibili", "B站"),
    ("douyin", "抖音"),
    ("tieba", "贴吧"),
    ("36kr", "36氪"),
    ("ithome", "IT之家"),
    ("sspai", "少数派"),
    ("juejin", "掘金"),
    ("wallstreetcn-hot", "华尔街见闻"),
    ("thepaper", "澎湃新闻"),
    ("bilibili-hot-search", "B站热搜"),
    ("cls-hot", "财联社"),
    ("ifeng", "凤凰网"),
];

pub struct DataFetcher {
    client: reqwest::Client,
    api_url: String,
}

impl DataFetcher {
    pub fn new(_proxy_url: Option<String>, api_url: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"));
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| TrendRadarError::Network(format!("build client: {}", e)))?;

        Ok(DataFetcher {
            client,
            api_url: api_url.unwrap_or_else(|| NEWSNOW_API_URL.to_string()),
        })
    }

    pub fn get_platform_ids() -> Vec<(String, String)> {
        PLATFORM_IDS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
    }

    pub async fn fetch_platform_raw(&self, platform_id: &str, max_retries: u32) -> Result<serde_json::Value> {
        let url = format!("{}?id={}&latest", self.api_url, platform_id);

        for attempt in 0..=max_retries {
            match self.client.get(&url).send().await {
                Ok(response) => {
                    let text = response.text().await
                        .map_err(|e| TrendRadarError::Network(format!("read body: {}", e)))?;
                    let json: serde_json::Value = serde_json::from_str(&text)
                        .map_err(|e| TrendRadarError::Parse(format!("json parse: {}", e)))?;

                    let status = json.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
                    match status {
                        "success" | "cache" => {
                            let label = if status == "success" { "最新数据" } else { "缓存数据" };
                            tracing::info!("获取 {} 成功（{}）", platform_id, label);
                            return Ok(json);
                        }
                        _ => {
                            tracing::warn!("{} 响应状态异常: {}", platform_id, status);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("请求 {} 失败: {}", platform_id, e);
                }
            }

            // 指数退避重试公式：3s + attempt*2s（单调递增）
            if attempt < max_retries {
                let wait_time = 3.0 + attempt as f64 * 2.0;
                tracing::info!("{:.2}秒后重试...", wait_time);
                tokio::time::sleep(Duration::from_secs_f64(wait_time)).await;
            }
        }

        Err(TrendRadarError::Network(format!("{} 所有重试均失败", platform_id)))
    }

    pub fn parse_platform_response(&self, json: &serde_json::Value, platform_id: &str, platform_name: &str) -> Vec<NewsItem> {
        let now = Utc::now();
        let items_array = json.get("items").and_then(|v| v.as_array());
        let items = match items_array {
            Some(arr) => arr,
            None => return vec![],
        };

        let mut seen_titles: HashMap<String, usize> = HashMap::new();

        items.iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let title = item.get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if title.is_empty() {
                    return None;
                }

                let rank = if let Some(entry) = seen_titles.get(&title) {
                    *entry as i32
                } else {
                    let rank = (index + 1) as i32;
                    seen_titles.insert(title.clone(), index + 1);
                    rank
                };

                let url = item.get("url").and_then(|u| u.as_str()).map(|s| s.to_string());
                let mobile_url = item.get("mobileUrl").and_then(|u| u.as_str()).map(|s| s.to_string());

                Some(NewsItem {
                    url: url.or(mobile_url),
                    title,
                    platform: platform_id.to_string(),
                    platform_name: Some(platform_name.to_string()),
                    rank: Some(rank),
                    hot_score: None,
                    summary: None,
                    author: None,
                    publish_time: None,
                    crawl_time: now,
                    category: None,
                    keywords: vec![],
                    is_new: None,
                    rank_change: None,
                    title_changed: None,
                    appearance_count: 0,
                    id: None,
                })
            })
            .collect()
    }

    pub async fn fetch_platform(&self, platform_id: &str, platform_name: &str, max_retries: u32) -> Result<Vec<NewsItem>> {
        let json = self.fetch_platform_raw(platform_id, max_retries).await?;
        Ok(self.parse_platform_response(&json, platform_id, platform_name))
    }

    pub async fn fetch_all_platforms(&self, platform_ids: &[(String, String)], request_interval_ms: u64, max_retries: u32) -> Result<Vec<NewsItem>> {
        let mut all_items = Vec::new();
        let total = platform_ids.len();

        for (i, (id, name)) in platform_ids.iter().enumerate() {
            tracing::info!("[{}/{}] 爬取 {} ({})", i + 1, total, name, id);

            match self.fetch_platform(id, name, max_retries).await {
                Ok(mut items) => {
                    tracing::info!("  {}: {} 条", name, items.len());
                    all_items.append(&mut items);
                }
                Err(e) => {
                    tracing::error!("  {}: 爬取失败 - {}", name, e);
                }
            }

            if i < total - 1 {
                let jitter: i64 = ((i as i64 * 13) % 30) - 10;
                let actual_interval = (request_interval_ms as i64 + jitter).max(50) as u64;
                tokio::time::sleep(Duration::from_millis(actual_interval)).await;
            }
        }

        Ok(all_items)
    }
}

pub struct RssFetcher {
    client: reqwest::Client,
    feeds: Vec<RssFeedConfig>,
    request_interval_ms: u64,
    #[allow(dead_code)]
    timeout_secs: u64,
    #[allow(dead_code)]
    freshness_enabled: bool,
    #[allow(dead_code)]
    default_max_age_days: i64,
}

#[derive(Debug, Clone)]
pub struct RssFeedConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub max_items: usize,
    pub enabled: bool,
    pub max_age_days: Option<i64>,
}

impl RssFetcher {
    pub fn new(feeds: Vec<RssFeedConfig>, request_interval_ms: u64, timeout_secs: u64, freshness_enabled: bool, default_max_age_days: i64) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("TrendRadar/2.0 RSS Reader (https://github.com/trendradar)"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/feed+json, application/json, application/rss+xml, application/atom+xml, application/xml, text/xml, */*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| TrendRadarError::Network(format!("build rss client: {}", e)))?;

        Ok(RssFetcher {
            client,
            feeds: feeds.into_iter().filter(|f| f.enabled).collect(),
            request_interval_ms,
            timeout_secs,
            freshness_enabled,
            default_max_age_days,
        })
    }

    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let rss = config.rss.as_ref();
        let feeds = rss.map(|r| {
            r.feeds.iter().map(|f| RssFeedConfig {
                id: f.name.clone().unwrap_or_else(|| f.url.clone()),
                name: f.name.clone().unwrap_or_else(|| f.url.clone()),
                url: f.url.clone(),
                max_items: 0,
                enabled: f.enabled,
                max_age_days: f.max_age_days,
            }).collect()
        }).unwrap_or_default();

        let request_interval = config.advanced.as_ref()
            .and_then(|a| a.request_delay)
            .unwrap_or(2000);
        let timeout = config.advanced.as_ref()
            .and_then(|a| a.request_timeout)
            .unwrap_or(15);

        Self::new(feeds, request_interval, timeout, true, 3)
    }

    pub async fn fetch_feed(&self, feed: &RssFeedConfig) -> (Vec<RssItem>, Option<String>) {
        match self.client.get(&feed.url).send().await {
            Ok(response) => {
                match response.text().await {
                    Ok(body) => {
                        match self.parse_xml(&body, feed) {
                            Ok(items) => {
                                tracing::info!("[RSS] {}: 获取 {} 条", feed.name, items.len());
                                (items, None)
                            }
                            Err(e) => (vec![], Some(format!("解析失败: {}", e))),
                        }
                    }
                    Err(e) => (vec![], Some(format!("读取响应失败: {}", e))),
                }
            }
            Err(e) => (vec![], Some(format!("请求失败: {}", e))),
        }
    }

    fn parse_xml(&self, xml: &str, feed: &RssFeedConfig) -> Result<Vec<RssItem>> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut items = Vec::new();
        let mut in_item = false;
        let mut current_tag = String::new();
        let mut title = String::new();
        let mut link = String::new();
        let mut description = String::new();
        let mut author = String::new();
        let mut pub_date = String::new();
        let mut guid = String::new();

        let mut buf = Vec::new();
        let now = Utc::now();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                    match tag.as_str() {
                        "item" | "entry" => in_item = true,
                        _ if in_item => current_tag = tag,
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                    match tag.as_str() {
                        "item" | "entry" => {
                            if !title.is_empty() {
                                let publish_time = parse_date(&pub_date);
                                items.push(RssItem {
                                    title: title.clone(),
                                    link: if link.is_empty() { None } else { Some(link.clone()) },
                                    description: if description.is_empty() { None } else { Some(description.clone()) },
                                    author: if author.is_empty() { None } else { Some(author.clone()) },
                                    publish_time,
                                    crawl_time: now,
                                    feed_name: feed.name.clone(),
                                    guid: {
                                        if !guid.is_empty() { Some(guid.clone()) }
                                        else if !link.is_empty() { Some(link.clone()) }
                                        else { Some(title.clone()) }
                                    },
                                    keywords: vec![],
                                    feed_id: None,
                                    summary: None,
                                    title_changed: false,
                                });
                            }
                            title.clear();
                            link.clear();
                            description.clear();
                            author.clear();
                            pub_date.clear();
                            guid.clear();
                            in_item = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if !in_item {
                        continue;
                    }
                    let text = e.unescape()
                        .unwrap_or_else(|_| String::from_utf8_lossy(e.as_ref()).into_owned().into())
                        .trim()
                        .to_string();

                    if !text.is_empty() {
                        match current_tag.as_str() {
                            "title" => { if title.is_empty() { title = text; } }
                            "link" => {
                                if link.is_empty() {
                                    link = text;
                                }
                            }
                            "id" if guid.is_empty() => guid = text,
                            "description" | "summary" | "content" => {
                                if description.is_empty() {
                                    description = text;
                                }
                            }
                            "author" | "name" => {
                                if author.is_empty() {
                                    author = text;
                                }
                            }
                            "published" | "pubdate" | "updated" => {
                                if pub_date.is_empty() {
                                    pub_date = text;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                    if tag == "link" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"href" {
                                link = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    tracing::warn!("XML parse error: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        if feed.max_items > 0 && items.len() > feed.max_items {
            items.truncate(feed.max_items);
        }

        Ok(items)
    }

    pub async fn fetch_all(&self) -> RssData {
        let mut all_items: Vec<RssItem> = Vec::new();
        let mut failed_ids = Vec::new();
        let total = self.feeds.len();
        let enabled_count = self.feeds.iter().filter(|f| f.enabled).count();

        tracing::info!("[RSS] 开始抓取 {}/{} 个已启用 RSS 源...", enabled_count, total);

        for (i, feed) in self.feeds.iter().filter(|f| f.enabled).enumerate() {
            if i > 0 {
                let jitter: f64 = ((i as f64 * 0.13) % 0.4) - 0.2;
                let interval = (self.request_interval_ms as f64 / 1000.0) + jitter * (self.request_interval_ms as f64 / 1000.0);
                tokio::time::sleep(Duration::from_secs_f64(interval)).await;
            }

            let (items, error) = self.fetch_feed(feed).await;

            if error.is_some() {
                failed_ids.push(feed.id.clone());
            } else {
                all_items.extend(items);
            }
        }

        let total_items = all_items.len();
        tracing::info!("[RSS] 抓取完成: {}/{} 源成功, 共 {} 条", total - failed_ids.len(), total, total_items);

        RssData {
            items: all_items,
            total: total_items,
            crawl_time: Utc::now(),
        }
    }
}

fn parse_date(s: &str) -> Option<chrono::DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let formats = [
        "%a, %d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M:%S %Z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
    ];

    for fmt in &formats {
        if let Ok(dt) = chrono::DateTime::parse_from_str(s, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc());
        }
    }

    None
}
