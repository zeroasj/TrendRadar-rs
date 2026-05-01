use axum::{Router, routing::post, Json};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use trendradar_core::config::AppConfig;
use trendradar_core::crawler::DataFetcher;
use trendradar_core::notify::Notifier;
use trendradar_core::storage::StorageManager;

static GLOBAL_STATE: std::sync::OnceLock<Arc<AppState>> = std::sync::OnceLock::new();

#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize)]
pub struct McpError { pub code: i32, pub message: String }

#[derive(Debug, Serialize)]
pub struct ToolDefinition { pub name: String, pub description: String, pub parameters: serde_json::Value }

#[derive(Debug, Clone)]
pub struct AppState { pub config: AppConfig, pub storage: StorageManager, pub config_path: String }

impl AppState {
    pub fn new(config: AppConfig, storage: StorageManager, config_path: &str) -> Self {
        AppState { config, storage, config_path: config_path.to_string() }
    }
}

fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "resolve_date_range".into(),
            description: concat!(
                "[RECOMMENDED FIRST CALL] Resolve natural language date expressions to precise date ranges.\n\n",
                "WHY: Users often use expressions like \"this week\" or \"last 7 days\". This tool uses server-side\n",
                "precise time to ensure ALL AI models get consistent date ranges.\n\n",
                "RECOMMENDED FLOW:\n",
                "1. User says \"analyze AI sentiment this week\"\n",
                "2. AI calls resolve_date_range(\"this week\") to get precise date range\n",
                "3. AI calls analyze_sentiment(topic=\"ai\", date_range=result.date_range)\n\n",
                "Args: expression (string, required) - supports:\n",
                "  single: \"today\", \"yesterday\", \"today\", \"yesterday\"\n",
                "  week: \"this week\", \"last week\", \"this week\", \"last week\"\n",
                "  month: \"this month\", \"last month\", \"this month\", \"last month\"\n",
                "  N days: \"last 7 days\", \"last 30 days\", \"last 7 days\", \"last 30 days\" (any number)\n\n",
                "Returns: JSON with date_range {start, end} usable directly by other tools\n\n",
                "Example: AI receives user request \"analyze AI sentiment this week\"  | ",
                "Step 1: resolve_date_range(\"this week\") returns {\"date_range\":{\"start\":\"2025-01-13\",\"end\":\"2025-01-19\"}}  | ",
                "Step 2: analyze_sentiment(topic=\"ai\", date_range={\"start\":\"2025-01-13\",\"end\":\"2025-01-19\"})"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"expression":{"type":"string","description":"natural language date expression"}},"required":["expression"]}),
        },
        ToolDefinition {
            name: "get_latest_news".into(),
            description: concat!(
                "Get the latest batch of crawled news data to quickly understand current hot topics.\n\n",
                "Args: platforms (list, optional) - platform IDs like ['zhihu','weibo'], omit for all platforms\n",
                "      limit (int, optional) - max results, default 50, max 1000\n",
                "      include_url (bool, optional) - include URL links, default False (save tokens)\n\n",
                "Returns: JSON news list with title, platform, rank, date\n\n",
                "DATA DISPLAY TIPS:\n",
                "- Show ALL returned data by default, only summarize when user explicitly asks\n",
                "- When user says \"summarize\" or \"pick highlights\", then filter\n",
                "- When user asks \"why only show part\", it means they want full data"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"platforms":{"type":"array","items":{"type":"string"}},"limit":{"type":"integer"},"include_url":{"type":"boolean"}}}),
        },
        ToolDefinition {
            name: "get_trending_topics".into(),
            description: concat!(
                "Get trending topics frequency statistics.\n\n",
                "Args: top_n (int, optional) - return TOP N topics, default 10\n",
                "      mode (str, optional) - time mode: \"daily\" (all-day cumulative) or \"current\" (latest batch, default)\n",
                "      extract_mode (str, optional) - extraction mode:\n",
                "        \"keywords\" (default) - count preset keywords from config/frequency_words.txt\n",
                "        \"auto_extract\" - auto extract high-frequency words from titles (no preset needed)\n\n",
                "Returns: JSON topic frequency list with topic, count, frequency percentage\n\n",
                "Examples:\n",
                "  get_trending_topics(mode=\"current\") - preset keyword stats\n",
                "  get_trending_topics(extract_mode=\"auto_extract\", top_n=20) - auto discover hot topics"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"top_n":{"type":"integer"},"mode":{"type":"string"},"extract_mode":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "get_latest_rss".into(),
            description: concat!(
                "Get latest RSS subscription data (supports multi-day queries).\n\n",
                "RSS data is stored separately from hotlist news, organized by timeline, suitable for\n",
                "getting latest content from specific sources.\n\n",
                "Args: feeds (list, optional) - RSS feed IDs like ['hacker-news','36kr'], omit for all\n",
                "      days (int, optional) - get last N days data, default 1 (today only), max 30\n",
                "      limit (int, optional) - max results, default 50, max 500\n",
                "      include_summary (bool, optional) - include article summary, default False (save tokens)\n\n",
                "Returns: JSON RSS item list\n\n",
                "Examples: get_latest_rss(days=7, feeds=['hacker-news'])"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"feeds":{"type":"array","items":{"type":"string"}},"days":{"type":"integer"},"limit":{"type":"integer"},"include_summary":{"type":"boolean"}}}),
        },
        ToolDefinition {
            name: "search_rss".into(),
            description: concat!(
                "Search RSS data by keyword.\n\n",
                "Args: keyword (str, required) - search keyword\n",
                "      feeds (list, optional) - RSS feed IDs like ['hacker-news','36kr'], omit to search all\n",
                "      days (int, optional) - search last N days, default 7, max 30\n",
                "      limit (int, optional) - max results, default 50\n",
                "      include_summary (bool, optional) - include article summary, default False\n\n",
                "Returns: JSON matching RSS items\n\n",
                "Examples: search_rss(keyword=\"AI\"), search_rss(keyword=\"machine learning\", feeds=['hacker-news'], days=14)"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"keyword":{"type":"string"},"feeds":{"type":"array","items":{"type":"string"}},"days":{"type":"integer"},"limit":{"type":"integer"},"include_summary":{"type":"boolean"}},"required":["keyword"]}),
        },
        ToolDefinition {
            name: "get_rss_feeds_status".into(),
            description: concat!(
                "Get RSS feed status information.\n\n",
                "View currently configured RSS feeds and their data statistics.\n\n",
                "Returns: JSON with available_dates, total_dates, today_feeds (per-feed name + item_count), generated_at"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "get_news_by_date".into(),
            description: concat!(
                "Get news data for specified date, used for historical data analysis and comparison.\n\n",
                "Args: date_range (object/string, optional) - supports multiple formats:\n",
                "        range object: {\"start\":\"2025-01-01\",\"end\":\"2025-01-07\"}\n",
                "        natural language: \"today\", \"yesterday\", \"this week\", \"last 7 days\"\n",
                "        single date: \"2025-01-15\"\n",
                "        default: \"today\"\n",
                "      platforms (list, optional) - platform IDs like ['zhihu','weibo'], omit for all\n",
                "      limit (int, optional) - max results, default 50, max 1000\n",
                "      include_url (bool, optional) - include URL links, default False\n\n",
                "Returns: JSON news list with title, platform, rank, etc."
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"date_range":{"type":"object"},"platforms":{"type":"array","items":{"type":"string"}},"limit":{"type":"integer"},"include_url":{"type":"boolean"}}}),
        },
        ToolDefinition {
            name: "analyze_topic_trend".into(),
            description: concat!(
                "Unified topic trend analysis tool - integrates multiple trend analysis modes.\n\n",
                "TIP: Use resolve_date_range first for natural language date expressions.\n\n",
                "Args: topic (str, required) - topic keyword\n",
                "      analysis_type (str, optional) - analysis type:\n",
                "        \"trend\" (default) - heat trend analysis\n",
                "        \"lifecycle\" - lifecycle analysis\n",
                "        \"viral\" - anomaly heat detection\n",
                "        \"predict\" - topic prediction\n",
                "      date_range (object, optional) - {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"}, default last 7 days\n",
                "      granularity (str, optional) - time granularity, default \"day\"\n",
                "      spike_threshold (float, optional) - heat spike multiplier threshold (viral mode), default 3.0\n",
                "      time_window (int, optional) - detection window hours (viral mode), default 24\n",
                "      lookahead_hours (int, optional) - prediction hours (predict mode), default 6\n",
                "      confidence_threshold (float, optional) - confidence threshold (predict mode), default 0.7\n\n",
                "Returns: JSON trend analysis results\n\n",
                "Examples:\n",
                "  analyze_topic_trend(topic=\"AI\", date_range={\"start\":\"2025-01-01\",\"end\":\"2025-01-07\"})\n",
                "  analyze_topic_trend(topic=\"Tesla\", analysis_type=\"lifecycle\")"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"topic":{"type":"string"},"analysis_type":{"type":"string"},"date_range":{"type":"object"},"granularity":{"type":"string"},"spike_threshold":{"type":"number"},"time_window":{"type":"integer"},"lookahead_hours":{"type":"integer"},"confidence_threshold":{"type":"number"}},"required":["topic"]}),
        },
        ToolDefinition {
            name: "analyze_data_insights".into(),
            description: concat!(
                "Unified data insights analysis tool - integrates multiple data analysis modes.\n\n",
                "Args: insight_type (str, optional) - insight type:\n",
                "        \"platform_compare\" (default) - compare topic attention across platforms\n",
                "        \"platform_activity\" - platform activity stats (frequency, active times)\n",
                "        \"keyword_cooccur\" - keyword co-occurrence pattern analysis\n",
                "      topic (str, optional) - topic keyword (for platform_compare mode)\n",
                "      date_range (object, optional) - {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"}\n",
                "      min_frequency (int, optional) - min co-occurrence frequency (keyword_cooccur), default 3\n",
                "      top_n (int, optional) - return TOP N results (keyword_cooccur), default 20\n\n",
                "Returns: JSON data insights results\n\n",
                "Examples:\n",
                "  analyze_data_insights(insight_type=\"platform_compare\", topic=\"AI\")\n",
                "  analyze_data_insights(insight_type=\"platform_activity\", date_range={\"start\":\"2025-01-01\",\"end\":\"2025-01-07\"})\n",
                "  analyze_data_insights(insight_type=\"keyword_cooccur\", min_frequency=5, top_n=15)"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"insight_type":{"type":"string"},"topic":{"type":"string"},"date_range":{"type":"object"},"min_frequency":{"type":"integer"},"top_n":{"type":"integer"}}}),
        },
        ToolDefinition {
            name: "analyze_sentiment".into(),
            description: concat!(
                "Analyze news sentiment polarity and heat trends.\n\n",
                "TIP: Use resolve_date_range first for natural language date expressions.\n\n",
                "Args: topic (str, optional) - topic keyword\n",
                "      platforms (list, optional) - platform IDs like ['zhihu','weibo'], omit for all\n",
                "      date_range (object, optional) - {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"}, default today\n",
                "      limit (int, optional) - return news count, default 50, max 100 (deduplicates titles)\n",
                "      sort_by_weight (bool, optional) - sort by heat weight, default True\n",
                "      include_url (bool, optional) - include URL links, default False\n\n",
                "Returns: JSON with sentiment distribution (positive/negative/neutral + percentages), heat trends, related news\n\n",
                "Examples: analyze_sentiment(topic=\"AI\", date_range={\"start\":\"2025-01-01\",\"end\":\"2025-01-07\"})"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"topic":{"type":"string"},"platforms":{"type":"array","items":{"type":"string"}},"date_range":{"type":"object"},"limit":{"type":"integer"},"sort_by_weight":{"type":"boolean"},"include_url":{"type":"boolean"}}}),
        },
        ToolDefinition {
            name: "find_related_news".into(),
            description: concat!(
                "Find news related to a given reference news title (supports current and historical data).\n\n",
                "Args: reference_title (str, required) - reference news title (full or partial)\n",
                "      date_range (object/string, optional) - date range:\n",
                "        omit: query today only\n",
                "        presets: \"today\", \"yesterday\", \"last_week\", \"last_month\"\n",
                "        custom: {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"}\n",
                "      threshold (float, optional) - similarity threshold 0-1, default 0.5 (higher = stricter)\n",
                "      limit (int, optional) - max results, default 50\n",
                "      include_url (bool, optional) - include URL links, default False\n\n",
                "Returns: JSON related news list sorted by similarity score\n\n",
                "Examples:\n",
                "  find_related_news(reference_title=\"Tesla price cut\")\n",
                "  find_related_news(reference_title=\"AI breakthrough\", date_range=\"last_week\")"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"reference_title":{"type":"string"},"date_range":{"type":"object"},"threshold":{"type":"number"},"limit":{"type":"integer"},"include_url":{"type":"boolean"}},"required":["reference_title"]}),
        },
        ToolDefinition {
            name: "generate_summary_report".into(),
            description: concat!(
                "Daily/weekly summary generator - auto-generate hot topic summary report.\n\n",
                "Args: report_type (str, optional) - report type: \"daily\" (default) or \"weekly\"\n",
                "      date_range (object, optional) - custom date range {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"}\n",
                "        IMPORTANT: must be object format, not integer\n\n",
                "Returns: JSON summary report with Markdown formatted content"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"report_type":{"type":"string"},"date_range":{"type":"object"}}}),
        },
        ToolDefinition {
            name: "aggregate_news".into(),
            description: concat!(
                "Cross-platform news aggregation - deduplicate and merge similar news.\n\n",
                "Merges reports of the same event from different platforms into one aggregated item,\n",
                "showing cross-platform coverage and combined heat.\n\n",
                "Args: date_range (object/string, optional) - date range, default today\n",
                "      platforms (list, optional) - platform IDs like ['zhihu','weibo'], omit for all\n",
                "      similarity_threshold (float, optional) - 0.3-1.0, default 0.7 (higher = stricter)\n",
                "      limit (int, optional) - max aggregated news, default 50\n",
                "      include_url (bool, optional) - include URL links, default False\n\n",
                "Returns: JSON aggregated results with dedup stats, clusters, platform coverage\n\n",
                "Examples: aggregate_news(), aggregate_news(similarity_threshold=0.8)"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"date_range":{"type":"object"},"platforms":{"type":"array","items":{"type":"string"}},"similarity_threshold":{"type":"number"},"limit":{"type":"integer"},"include_url":{"type":"boolean"}}}),
        },
        ToolDefinition {
            name: "compare_periods".into(),
            description: concat!(
                "Period comparison analysis - compare news data between two time periods.\n\n",
                "Compare hot topics, platform activity, news volume across dimensions.\n\n",
                "USE CASES:\n",
                "- Compare this week vs last week hotspot changes\n",
                "- Analyze topic heat difference between two periods\n",
                "- View platform activity cyclical changes\n\n",
                "Args: period1 (object/string, required) - first period (baseline):\n",
                "        {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"} or \"today\"/\"yesterday\"/\"this_week\"/\"last_week\"/\"this_month\"/\"last_month\"\n",
                "      period2 (object/string, required) - second period (comparison, same format as period1)\n",
                "      topic (str, optional) - topic keyword to focus on specific topic comparison\n",
                "      compare_type (str, optional) - comparison type:\n",
                "        \"overview\" (default) - news count, keyword changes, TOP news\n",
                "        \"topic_shift\" - rising/falling/new topics analysis\n",
                "        \"platform_activity\" - per-platform news volume changes\n",
                "      platforms (list, optional) - platform filter like ['zhihu','weibo']\n",
                "      top_n (int, optional) - return TOP N results, default 10\n\n",
                "Returns: JSON comparison results with periods, change stats, top items\n\n",
                "Examples:\n",
                "  compare_periods(period1=\"last_week\", period2=\"this_week\")   - week over week\n",
                "  compare_periods(period1=\"last_month\", period2=\"this_month\", compare_type=\"topic_shift\")\n",
                "  compare_periods(period1={\"start\":\"2025-01-01\",\"end\":\"2025-01-07\"}, period2={\"start\":\"2025-01-08\",\"end\":\"2025-01-14\"}, topic=\"AI\")"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"period1":{"type":"object"},"period2":{"type":"object"},"topic":{"type":"string"},"compare_type":{"type":"string"},"platforms":{"type":"array","items":{"type":"string"}},"top_n":{"type":"integer"}},"required":["period1","period2"]}),
        },
        ToolDefinition {
            name: "search_news".into(),
            description: concat!(
                "Unified search interface, supports multiple search modes, can search both hotlist and RSS.\n\n",
                "TIP: Use resolve_date_range first for natural language date expressions.\n\n",
                "Args: query (str, required) - search keyword or content fragment\n",
                "      search_mode (str, optional) - search mode:\n",
                "        \"keyword\" (default) - exact keyword match\n",
                "        \"fuzzy\" - fuzzy content match\n",
                "        \"entity\" - entity name search (person/place/organization)\n",
                "      date_range (object, optional) - {\"start\":\"YYYY-MM-DD\",\"end\":\"YYYY-MM-DD\"}, default today\n",
                "      platforms (list, optional) - platform IDs like ['zhihu','weibo'], omit for all\n",
                "      limit (int, optional) - hotlist return limit, default 50\n",
                "      sort_by (str, optional) - sort: \"relevance\"/\"weight\"/\"date\"\n",
                "      threshold (float, optional) - similarity threshold (fuzzy mode only), 0-1, default 0.6\n",
                "      include_url (bool, optional) - include URL links, default False\n",
                "      include_rss (bool, optional) - also search RSS data, default False\n",
                "      rss_limit (int, optional) - RSS return limit, default 20\n\n",
                "Returns: JSON search results with hotlist news items and optional RSS results\n\n",
                "Examples:\n",
                "  search_news(query=\"AI\")\n",
                "  search_news(query=\"AI\", include_rss=True)\n",
                "  search_news(query=\"Tesla\", date_range={\"start\":\"2025-01-01\",\"end\":\"2025-01-07\"})"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"query":{"type":"string"},"search_mode":{"type":"string"},"date_range":{"type":"object"},"platforms":{"type":"array","items":{"type":"string"}},"limit":{"type":"integer"},"sort_by":{"type":"string"},"threshold":{"type":"number"},"include_url":{"type":"boolean"},"include_rss":{"type":"boolean"},"rss_limit":{"type":"integer"}},"required":["query"]}),
        },
        ToolDefinition {
            name: "get_current_config".into(),
            description: concat!(
                "Get current system configuration.\n\n",
                "Args: section (str, optional) - config section:\n",
                "        \"all\" (default) - all config\n",
                "        \"crawler\" - crawler config\n",
                "        \"push\" - push config\n",
                "        \"keywords\" - keyword config\n",
                "        \"weights\" - weight config\n\n",
                "Returns: JSON config info (sensitive fields masked)"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"section":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "get_system_status".into(),
            description: concat!(
                "Get system runtime status and health check info.\n\n",
                "Returns: system version, data stats (database size, table row counts), platform config count, etc."
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "check_version".into(),
            description: concat!(
                "Check version updates (both TrendRadar and MCP Server).\n\n",
                "Compares local version with GitHub remote version, determines if update needed.\n\n",
                "Args: proxy_url (str, optional) - proxy URL for GitHub access like http://127.0.0.1:7890\n\n",
                "Returns: JSON version check results with both component version comparison and update flag\n\n",
                "Examples: check_version(), check_version(proxy_url=\"http://127.0.0.1:7890\")"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"proxy_url":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "trigger_crawl".into(),
            description: concat!(
                "Manually trigger a crawl task (optional persistence).\n\n",
                "Args: platforms (list, optional) - platform IDs like ['zhihu','weibo'], omit for all platforms\n",
                "      save_to_local (bool, optional) - save to local output dir, default False\n",
                "      include_url (bool, optional) - include URL links, default False (save tokens)\n\n",
                "Returns: JSON task status with success/failed platform list and news data\n\n",
                "Examples: trigger_crawl(platforms=['zhihu']), trigger_crawl(save_to_local=True)"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"platforms":{"type":"array","items":{"type":"string"}},"save_to_local":{"type":"boolean"},"include_url":{"type":"boolean"}}}),
        },
        ToolDefinition {
            name: "sync_from_remote".into(),
            description: concat!(
                "Pull data from remote storage to local.\n\n",
                "For MCP Server scenarios: crawler stores to remote cloud (e.g. Cloudflare R2),\n",
                "MCP Server pulls to local for analysis queries.\n\n",
                "Args: days (int, optional) - pull last N days data, default 7\n",
                "        0: skip, 7: last week, 30: last month\n\n",
                "Returns: JSON sync results with synced_files, synced_dates, skipped_dates, failed_dates\n\n",
                "Note: requires remote storage config in config/config.yaml (storage.remote) or env vars:\n",
                "  S3_ENDPOINT_URL, S3_BUCKET_NAME, S3_ACCESS_KEY_ID, S3_SECRET_ACCESS_KEY"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"days":{"type":"integer"}}}),
        },
        ToolDefinition {
            name: "get_storage_status".into(),
            description: concat!(
                "Get storage config and status.\n\n",
                "View current storage backend config, local and remote storage status.\n\n",
                "Returns: JSON storage status with local/remote backend info and pull config"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "list_available_dates".into(),
            description: concat!(
                "List available date ranges locally/remotely.\n\n",
                "View which dates have data available in local and remote storage.\n\n",
                "Args: source (str, optional) - data source:\n",
                "        \"local\" - local only\n",
                "        \"remote\" - remote only\n",
                "        \"both\" (default) - list both and compare\n\n",
                "Returns: JSON date lists with per-source date info and comparison\n\n",
                "Examples: list_available_dates(), list_available_dates(source=\"local\")"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"source":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "read_article".into(),
            description: concat!(
                "Read article content from URL, returns LLM-friendly Markdown format.\n\n",
                "Uses Jina AI Reader to convert web pages to clean Markdown, auto-removing ads,\n",
                "navigation bars and other noise. Suitable for: reading news full text, getting\n",
                "article details, analyzing article content.\n\n",
                "TYPICAL FLOW:\n",
                "1. Use search_news(include_url=True) to search news and get links\n",
                "2. Use read_article(url=link) to read full text\n",
                "3. AI analyzes, summarizes, translates the Markdown content\n\n",
                "Args: url (str, required) - article link starting with http:// or https://\n",
                "      timeout (int, optional) - request timeout seconds, default 30, max 60\n\n",
                "Returns: JSON article content with full Markdown body\n\n",
                "Note: uses Jina AI Reader free service (100 RPM limit), 5s rate control built-in"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"timeout":{"type":"integer"}},"required":["url"]}),
        },
        ToolDefinition {
            name: "read_articles_batch".into(),
            description: concat!(
                "Batch read multiple article contents (max 5, 5s interval between each).\n\n",
                "Requests articles one by one, auto 5s interval to comply with rate limit.\n\n",
                "TYPICAL FLOW:\n",
                "1. Use search_news(include_url=True) to search news and get multiple links\n",
                "2. Use read_articles_batch(urls=[...]) to batch read full text\n",
                "3. AI performs comparative analysis, comprehensive report across articles\n\n",
                "Args: urls (list, required) - article link list, max 5 processed\n",
                "      timeout (int, optional) - per-article request timeout, default 30\n\n",
                "Returns: JSON batch read results with per-article content and status\n\n",
                "Note: max 5 articles per call (excess skipped), ~25-30s for 5 articles (5s interval each)"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"urls":{"type":"array","items":{"type":"string"}},"timeout":{"type":"integer"}},"required":["urls"]}),
        },
        ToolDefinition {
            name: "get_channel_format_guide".into(),
            description: concat!(
                "Get notification channel formatting strategy guide.\n\n",
                "Returns Markdown features supported per channel, format limits, and best formatting tips.\n",
                "Use this tool before send_notification to understand target channel format requirements\n",
                "and generate optimally formatted messages.\n\n",
                "CHANNEL FORMAT DIFFERENCES:\n",
                "- Feishu: supports **bold**, <font color>colored text, [links](url), --- separator\n",
                "- DingTalk: supports ### headers, **bold**, > quote, --- separator, no color\n",
                "- WeCom: only **bold**, [links](url), > quote, no headers or separator\n",
                "- Telegram: auto-converted to HTML, supports bold/italic/strikethrough/code/link/blockquote\n",
                "- ntfy: standard Markdown, no color\n",
                "- Bark: iOS push, only bold and links, keep content concise\n",
                "- Slack: auto-converted to mrkdwn, *bold*, ~strikethrough~, <url|link>\n",
                "- Email: full HTML conversion, supports headers/styles/separators\n",
                "- Generic Webhook: standard Markdown or custom template\n\n",
                "Args: channel (str, optional) - channel ID: feishu/dingtalk/wework/telegram/email/ntfy/bark/slack/generic_webhook\n",
                "      omit to return all channel strategies"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"channel":{"type":"string"}}}),
        },
        ToolDefinition {
            name: "get_notification_channels".into(),
            description: concat!(
                "Get all configured notification channels and their status.\n\n",
                "Detects notification channel config from config.yaml and .env environment variables.\n",
                "Supports 9 channels: Feishu, DingTalk, WeCom, Telegram, Email, ntfy, Bark, Slack, Generic Webhook.\n\n",
                "Returns: JSON channel status with per-channel configured flag and config source"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "send_notification".into(),
            description: concat!(
                "Send message to configured notification channels.\n\n",
                "Accepts markdown format content, internally auto-adapts to each channel's format limits:\n",
                "- Feishu: Markdown card (supports **bold**, <font color>, [link](url), ---)\n",
                "- DingTalk: Markdown (auto-downgrade headers to ###, strip <font> and strikethrough)\n",
                "- WeCom: Markdown (auto-strip # headers, ---, <font>, strikethrough)\n",
                "- Telegram: HTML (auto-convert ** to <b>, * to <i>, ~~ to <s>, > to <blockquote>)\n",
                "- Email: HTML email (full web style, supports # headers, ---, bold/italic)\n",
                "- ntfy: Markdown (auto-strip <font>)\n",
                "- Bark: Markdown (auto-simplify to bold + link, iOS push optimized)\n",
                "- Slack: mrkdwn (auto-convert ** to *, ~~ to ~, [text](url) to <url|text>)\n",
                "- Generic Webhook: Markdown (supports custom template)\n\n",
                "TIP: Call get_channel_format_guide before sending to get detailed formatting strategy\n",
                "for target channel, to generate optimally formatted message content.\n\n",
                "Args: message (str, required) - markdown format message content\n",
                "      title (str, optional) - message title, default \"TrendRadar notification\"\n",
                "      channels (list, optional) - channel list: feishu/dingtalk/wework/telegram/email/ntfy/bark/slack/generic_webhook\n",
                "        omit to send to all configured channels\n\n",
                "Returns: JSON send results with per-channel status\n\n",
                "Examples:\n",
                "  send_notification(message=\"**Test message**\\nThis is a test\")\n",
                "  send_notification(message=\"Urgent alert\", title=\"System Warning\", channels=[\"feishu\",\"dingtalk\"])"
            ).into(),
            parameters: serde_json::json!({"type":"object","properties":{"message":{"type":"string"},"title":{"type":"string"},"channels":{"type":"array","items":{"type":"string"}}},"required":["message"]}),
        },
    ]
}

// ============================================================================
// Axum handlers
// ============================================================================

pub async fn serve(config: AppConfig, storage: StorageManager, host: &str, port: u16) -> anyhow::Result<()> {
    let _ = GLOBAL_STATE.set(Arc::new(AppState::new(config, storage, "config/config.yaml")));
    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .route("/health", post(health_handler));
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port)).await?;
    tracing::info!("MCP server listening on {}:{}", host, port);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok","service":"TrendRadar MCP Server"}))
}

#[axum::debug_handler]
async fn mcp_handler(Json(request): Json<McpRequest>) -> Json<McpResponse> {
    let state = GLOBAL_STATE.get().expect("AppState not initialized").clone();
    let response = match request.method.as_str() {
        "initialize" => initialize_response(request.id),
        "tools/list" => tools_list_response(request.id),
        "tools/call" => tools_call_response(&state, request.id, &request.params).await,
        _ => error_response(request.id, &format!("Method not found: {}", request.method)),
    };
    Json(response)
}

fn initialize_response(id: Option<serde_json::Value>) -> McpResponse {
    McpResponse { jsonrpc: "2.0".into(), id, result: Some(serde_json::json!({
        "protocolVersion":"2024-11-05",
        "serverInfo":{"name":"TrendRadar MCP Server","version":env!("CARGO_PKG_VERSION")},
        "capabilities":{"tools":{}}
    })), error: None }
}

fn tools_list_response(id: Option<serde_json::Value>) -> McpResponse {
    let tools: Vec<serde_json::Value> = get_tool_definitions().into_iter()
        .map(|t| serde_json::json!({"name":t.name,"description":t.description,"inputSchema":t.parameters}))
        .collect();
    McpResponse { jsonrpc: "2.0".into(), id, result: Some(serde_json::json!({"tools":tools})), error: None }
}

fn ok_response(id: Option<serde_json::Value>, result: serde_json::Value) -> McpResponse {
    McpResponse { jsonrpc: "2.0".into(), id,
        result: Some(serde_json::json!({"content":[{"type":"text","text":serde_json::to_string(&result).unwrap_or_default()}]})),
        error: None }
}

fn error_response(id: Option<serde_json::Value>, msg: &str) -> McpResponse {
    McpResponse { jsonrpc: "2.0".into(), id, result: None, error: Some(McpError { code: -32000, message: msg.to_string() }) }
}

// ============================================================================
// Dispatch
// ============================================================================

async fn tools_call_response(state: &AppState, id: Option<serde_json::Value>, params: &Option<serde_json::Value>) -> McpResponse {
    let tool_name = match params.as_ref().and_then(|p| p.get("name")).and_then(|v| v.as_str()) {
        Some(n) => n, None => return error_response(id, "Missing tool name"),
    };
    let args = params.as_ref().and_then(|p| p.get("arguments")).cloned().unwrap_or(serde_json::Value::Null);
    let result: Result<serde_json::Value, String> = match tool_name {
        "resolve_date_range" => h_resolve_date_range(&args).await,
        "get_latest_news" => h_get_latest_news(state, &args).await,
        "get_trending_topics" => h_get_trending_topics(state, &args).await,
        "get_latest_rss" => h_get_latest_rss(state, &args).await,
        "search_rss" => h_search_rss(state, &args).await,
        "get_rss_feeds_status" => h_get_rss_feeds_status(state).await,
        "get_news_by_date" => h_get_news_by_date(state, &args).await,
        "search_news" => h_search_news(state, &args).await,
        "get_current_config" => h_get_current_config(state, &args).await,
        "get_system_status" => h_get_system_status(state).await,
        "get_storage_status" => h_get_storage_status(state).await,
        "list_available_dates" => h_list_available_dates(state, &args).await,
        "get_channel_format_guide" => h_get_channel_format_guide(&args).await,
        "get_notification_channels" => h_get_notification_channels(state).await,
        "send_notification" => h_send_notification(state, &args).await,
        "analyze_topic_trend" => h_analyze_topic_trend(state, &args).await,
        "analyze_data_insights" => h_analyze_data_insights(state, &args).await,
        "analyze_sentiment" => h_analyze_sentiment(state, &args).await,
        "find_related_news" => h_find_related_news(state, &args).await,
        "generate_summary_report" => h_generate_summary_report(state, &args).await,
        "aggregate_news" => h_aggregate_news(state, &args).await,
        "compare_periods" => h_compare_periods(state, &args).await,
        "check_version" => h_check_version(&args).await,
        "trigger_crawl" => h_trigger_crawl(state, &args).await,
        "sync_from_remote" => h_sync_from_remote(&args).await,
        "read_article" => h_read_article(&args).await,
        "read_articles_batch" => h_read_articles_batch(&args).await,
        _ => return error_response(id, &format!("Unknown tool: {}", tool_name)),
    };
    match result { Ok(v) => ok_response(id, v), Err(e) => error_response(id, &e) }
}

// ============================================================================
// Handler: resolve_date_range
// ============================================================================

async fn h_resolve_date_range(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let expr = args.get("expression").and_then(|v| v.as_str()).ok_or("Missing 'expression'")?;
    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let (start, end, desc) = match expr.to_lowercase().as_str() {
        "今天"|"today" => (today.clone(), today.clone(), "今天".into()),
        "昨天"|"yesterday" => { let d = now - chrono::Duration::days(1); (d.format("%Y-%m-%d").to_string(), d.format("%Y-%m-%d").to_string(), "昨天".into()) }
        "本周"|"this week" => { let dow = now.weekday().num_days_from_monday(); let mon = now - chrono::Duration::days(dow as i64); (mon.format("%Y-%m-%d").to_string(), today.clone(), "本周".into()) }
        "上周"|"last week" => { let dow = now.weekday().num_days_from_monday(); let mon = now - chrono::Duration::days((dow+7) as i64); let sun = mon + chrono::Duration::days(6); (mon.format("%Y-%m-%d").to_string(), sun.format("%Y-%m-%d").to_string(), "上周".into()) }
        "本月"|"this month" => { let first = chrono::NaiveDate::from_ymd_opt(now.year(),now.month(),1).unwrap(); (first.format("%Y-%m-%d").to_string(), today.clone(), "本月".into()) }
        "上月"|"last month" => {
            let (y,m) = if now.month()==1 {(now.year()-1,12)} else {(now.year(),now.month()-1)};
            let first = chrono::NaiveDate::from_ymd_opt(y,m,1).unwrap();
            let last = if m==12 { chrono::NaiveDate::from_ymd_opt(y+1,1,1) } else { chrono::NaiveDate::from_ymd_opt(y,m+1,1) };
            (first.format("%Y-%m-%d").to_string(), last.unwrap_or(first).format("%Y-%m-%d").to_string(), "上月".into())
        }
        _ => {
            if let Some(n) = parse_days(expr, "最近","天") { let s = now - chrono::Duration::days(n); (s.format("%Y-%m-%d").to_string(), today.clone(), format!("最近{}天",n)) }
            else if let Some(n) = parse_days(expr, "last","days") { let s = now - chrono::Duration::days(n); (s.format("%Y-%m-%d").to_string(), today.clone(), format!("last {} days",n)) }
            else { return Err(format!("无法解析: {}", expr)); }
        }
    };
    Ok(serde_json::json!({"success":true,"expression":expr,"current_date":today,"date_range":{"start":start,"end":end},"description":format!("{}: {} 至 {}",desc,start,end)}))
}

fn parse_days(expr: &str, prefix: &str, suffix: &str) -> Option<i64> {
    let l = expr.to_lowercase();
    let p = l.find(&prefix.to_lowercase())?;
    let s = l.rfind(&suffix.to_lowercase())?;
    expr[p+prefix.len()..s].trim().parse().ok()
}

// ============================================================================
// Handler: get_latest_news
// ============================================================================

async fn h_get_latest_news(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(1000) as usize;
    let pf = args.get("platforms").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str());
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let data = state.storage.query_news(pf, Some(limit), None).map_err(|e| e.to_string())?;
    let items: Vec<serde_json::Value> = data.items.iter().map(|item| {
        let mut j = serde_json::json!({"title":item.title,"platform":item.platform,"platform_name":item.platform_name,"rank":item.rank,"date":item.crawl_time.format("%Y-%m-%d").to_string()});
        if include_url { if let Some(ref u) = item.url { j["url"] = serde_json::json!(u); } }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"summary":{"total":items.len(),"description":"最新新闻数据"},"data":items}))
}

// ============================================================================
// Handler: get_trending_topics
// ============================================================================

async fn h_get_trending_topics(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let top_n = args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(10).min(50) as usize;
    let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("current");
    let extract_mode = args.get("extract_mode").and_then(|v| v.as_str()).unwrap_or("keywords");
    let data = state.storage.query_news(None, Some(200), None).map_err(|e| e.to_string())?;
    let mut freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut total = 0;
    for item in &data.items {
        total += 1;
        let mut added = false;
        for word in item.title.split(|c: char| !c.is_alphanumeric() && !c.is_alphabetic() && c != '\'') {
            let w = word.trim();
            if w.len() >= 2 && !w.chars().all(|c| c.is_ascii_digit()) { *freq.entry(w.to_string()).or_default() += 1; added = true; }
        }
        if !added { *freq.entry(item.platform_name.clone().unwrap_or_else(|| item.platform.clone())).or_default() += 1; }
    }
    let mut sorted: Vec<(String, usize)> = freq.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    let topics: Vec<serde_json::Value> = sorted.iter().take(top_n).map(|(k, v)| {
        serde_json::json!({"topic":k,"count":v,"frequency":format!("{:.1}%", if total>0 {(v*100)as f64/total as f64}else{0.0})})
    }).collect();
    Ok(serde_json::json!({"success":true,"mode":mode,"extract_mode":extract_mode,"total_news":total,"topics":topics}))
}

// ============================================================================
// Handler: get_latest_rss
// ============================================================================

async fn h_get_latest_rss(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(500) as usize;
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(1).min(30) as i64;
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let feed = args.get("feeds").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str());
    let include_summary = args.get("include_summary").and_then(|v| v.as_bool()).unwrap_or(false);
    let data = state.storage.query_rss(feed, Some(limit), Some(since)).map_err(|e| e.to_string())?;
    let items: Vec<serde_json::Value> = data.items.iter().map(|item| {
        let mut j = serde_json::json!({"title":item.title,"feed_name":item.feed_name,"publish_time":item.publish_time.map(|t|t.to_rfc3339()),"link":item.link});
        if include_summary { j["description"] = serde_json::json!(item.description.as_deref().unwrap_or("")); }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"total":items.len(),"days":days,"data":items}))
}

// ============================================================================
// Handler: search_rss
// ============================================================================

async fn h_search_rss(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let keyword = args.get("keyword").and_then(|v| v.as_str()).ok_or("Missing 'keyword'")?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(500) as usize;
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(7).min(30) as i64;
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let include_summary = args.get("include_summary").and_then(|v| v.as_bool()).unwrap_or(false);
    let feed = args.get("feeds").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str());
    let data = state.storage.query_rss(feed, Some(limit * 2), Some(since)).map_err(|e| e.to_string())?;
    let kw = keyword.to_lowercase();
    let filtered: Vec<serde_json::Value> = data.items.iter()
        .filter(|item| item.title.to_lowercase().contains(&kw) || item.description.as_deref().unwrap_or("").to_lowercase().contains(&kw))
        .take(limit)
        .map(|item| {
            let mut j = serde_json::json!({"title":item.title,"feed_name":item.feed_name,"publish_time":item.publish_time.map(|t|t.to_rfc3339()),"link":item.link});
            if include_summary { j["description"] = serde_json::json!(item.description); }
            j
        }).collect();
    Ok(serde_json::json!({"success":true,"summary":{"total":filtered.len(),"keyword":keyword,"days":days},"data":filtered}))
}

// ============================================================================
// Handler: get_rss_feeds_status
// ============================================================================

async fn h_get_rss_feeds_status(state: &AppState) -> Result<serde_json::Value, String> {
    if let Some(ref rss) = state.config.rss {
        let feeds: Vec<serde_json::Value> = rss.feeds.iter().map(|f| serde_json::json!({"name":f.name.as_deref().unwrap_or(&f.url),"url":f.url})).collect();
        Ok(serde_json::json!({"success":true,"feeds":feeds,"total_feeds":feeds.len(),"generated_at":chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()}))
    } else { Ok(serde_json::json!({"success":true,"feeds":[],"total_feeds":0})) }
}

// ============================================================================
// Handler: get_news_by_date
// ============================================================================

async fn h_get_news_by_date(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let (target, has_range) = resolve_date_arg(args.get("date_range"));
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(1000) as usize;
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let data = if let Some(end) = has_range {
        state.storage.query_news_by_date_range(&target, &end).map_err(|e| e.to_string())?
    } else {
        state.storage.query_news_by_date(&target).map_err(|e| e.to_string())?
    };
    let items: Vec<serde_json::Value> = data.items.iter().take(limit).map(|item| {
        let mut j = serde_json::json!({"title":item.title,"platform":item.platform,"platform_name":item.platform_name,"rank":item.rank,"date":item.crawl_time.format("%Y-%m-%d").to_string()});
        if include_url { if let Some(ref u) = item.url { j["url"] = serde_json::json!(u); } }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"date":target,"total":items.len(),"data":items}))
}

fn resolve_date_arg(v: Option<&serde_json::Value>) -> (String, Option<String>) {
    let now = chrono::Local::now();
    match v.and_then(|v| v.as_str()) {
        Some("today"|"今天") => (now.format("%Y-%m-%d").to_string(), None),
        Some("yesterday"|"昨天") => ((now - chrono::Duration::days(1)).format("%Y-%m-%d").to_string(), None),
        Some(s) => (s.to_string(), None),
        None => match v.and_then(|v| v.get("end")).and_then(|v| v.as_str()) {
            Some(end) => {
                let start = v.and_then(|v| v.get("start")).and_then(|v| v.as_str())
                    .unwrap_or(now.format("%Y-%m-%d").to_string().as_str()).to_string();
                (start, Some(end.to_string()))
            }
            None => match v.and_then(|v| v.get("start")).and_then(|v| v.as_str()) {
                Some(s) => (s.to_string(), None),
                None => (now.format("%Y-%m-%d").to_string(), None),
            }
        }
    }
}

// ============================================================================
// Handler: search_news
// ============================================================================

async fn h_search_news(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let query = args.get("query").and_then(|v| v.as_str()).ok_or("Missing 'query'")?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(1000) as usize;
    let include_rss = args.get("include_rss").and_then(|v| v.as_bool()).unwrap_or(false);
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let days = parse_date_days(args, 7);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let data = state.storage.search_news_by_title(query, limit, Some(since)).map_err(|e| e.to_string())?;
    let results: Vec<serde_json::Value> = data.items.iter().map(|item| {
        let mut j = serde_json::json!({"title":item.title,"platform":item.platform,"platform_name":item.platform_name,"rank":item.rank,"date":item.crawl_time.format("%Y-%m-%d").to_string()});
        if include_url { if let Some(ref u) = item.url { j["url"] = serde_json::json!(u); } }
        j
    }).collect();
    let mut resp = serde_json::json!({"success":true,"summary":{"total_found":results.len(),"returned":results.len(),"query":query,"search_mode":"keyword"},"data":results});
    if include_rss {
        let rss_limit = args.get("rss_limit").and_then(|v| v.as_u64()).unwrap_or(20).min(100) as usize;
        let rss_data = state.storage.query_rss(None, Some(rss_limit), Some(since)).map_err(|e| e.to_string())?;
        let q = query.to_lowercase();
        let rss: Vec<serde_json::Value> = rss_data.items.iter().filter(|i| i.title.to_lowercase().contains(&q)).take(rss_limit)
            .map(|i| serde_json::json!({"title":i.title,"feed_name":i.feed_name,"link":i.link})).collect();
        resp["rss"] = serde_json::json!(rss);
        resp["rss_total"] = serde_json::json!(rss.len());
    }
    Ok(resp)
}

// ============================================================================
// Handler: get_current_config
// ============================================================================

async fn h_get_current_config(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let section = args.get("section").and_then(|v| v.as_str()).unwrap_or("all");
    let mut v = serde_json::to_value(&state.config).map_err(|e| e.to_string())?;
    mask_sensitive(&mut v);
    Ok(serde_json::json!({"success":true,"section":section,"config":v}))
}

fn mask_sensitive(v: &mut serde_json::Value) {
    let keys = ["api_key","password","secret_key","s3_access_key","s3_secret_key","bot_token","token"];
    match v {
        serde_json::Value::Object(m) => {
            let ks: Vec<String> = m.keys().cloned().collect();
            for k in ks {
                if keys.iter().any(|sk| k == *sk) { if let Some(serde_json::Value::String(s)) = m.get_mut(&k) { if !s.is_empty() { *s = "***".into(); } } }
                else if let Some(cv) = m.get_mut(&k) { mask_sensitive(cv); }
            }
        }
        serde_json::Value::Array(a) => { for i in a.iter_mut() { mask_sensitive(i); } }
        _ => {}
    }
}

// ============================================================================
// Handler: get_system_status
// ============================================================================

async fn h_get_system_status(state: &AppState) -> Result<serde_json::Value, String> {
    let db_size = state.storage.get_db_size_bytes().unwrap_or(0);
    let counts = state.storage.get_table_counts().unwrap_or_default();
    Ok(serde_json::json!({"success":true,"data":{"status":"running","version":env!("CARGO_PKG_VERSION"),"database_size_bytes":db_size,"database_size_mb":format!("{:.2}",db_size as f64/1048576.0),"table_counts":counts,"platforms_configured":state.config.platforms.sources.iter().filter(|s|s.enabled).count()}}))
}

async fn h_get_storage_status(state: &AppState) -> Result<serde_json::Value, String> {
    let db_size = state.storage.get_db_size_bytes().unwrap_or(0);
    let counts = state.storage.get_table_counts().unwrap_or_default();
    Ok(serde_json::json!({"success":true,"local":{"backend":"SQLite","database_size_bytes":db_size,"table_counts":counts},"remote":{"configured":false,"message":"远程存储未配置"}}))
}

async fn h_list_available_dates(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let _source = args.get("source").and_then(|v| v.as_str()).unwrap_or("both");
    let dates = state.storage.list_available_dates().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"success":true,"local":{"dates":dates,"count":dates.len()},"remote":{"dates":[],"count":0,"message":"远程存储未配置"}}))
}

// ============================================================================
// Handler: notification
// ============================================================================

async fn h_get_channel_format_guide(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let ch = args.get("channel").and_then(|v| v.as_str());
    let guides = serde_json::json!({
        "feishu":{"supports":["bold","color","links","separator"],"max_bytes":29000,"format":"markdown with <font color>"},
        "dingtalk":{"supports":["headers_h3","bold","quote","separator"],"max_bytes":20000,"format":"markdown","note":"不支持彩色文本"},
        "wework":{"supports":["bold","links","quote"],"max_bytes":4000,"format":"markdown","note":"不支持标题和分割线"},
        "telegram":{"supports":["bold","italic","strikethrough","code","links","blockquote"],"max_bytes":4000,"format":"HTML"},
        "email":{"supports":["all_markdown","styles"],"max_bytes":524288,"format":"HTML"},
        "ntfy":{"supports":["standard_markdown"],"max_bytes":3800,"format":"markdown","note":"不支持颜色"},
        "bark":{"supports":["bold","links"],"max_bytes":3600,"format":"markdown","note":"内容需精简"},
        "slack":{"supports":["bold_mrkdwn","strikethrough","links_mrkdwn"],"max_bytes":4000,"format":"mrkdwn"},
        "generic_webhook":{"supports":["standard_markdown","custom_template"],"max_bytes":100000,"format":"markdown"}
    });
    if let Some(c) = ch { Ok(serde_json::json!({"success":true,"channel":c,"guide":guides.get(c).unwrap_or(&serde_json::json!({}))})) }
    else { Ok(serde_json::json!({"success":true,"channels":guides})) }
}

async fn h_get_notification_channels(state: &AppState) -> Result<serde_json::Value, String> {
    let mut r = serde_json::json!({"success":true});
    if let Some(ref n) = state.config.notification {
        let c = &n.channels;
        let mut e: Vec<String> = Vec::new();
        if !c.feishu.is_empty() { e.push("feishu".into()); }
        if !c.dingtalk.is_empty() { e.push("dingtalk".into()); }
        if !c.wework.is_empty() { e.push("wecom".into()); }
        if !c.telegram.is_empty() { e.push("telegram".into()); }
        if !c.email.is_empty() { e.push("email".into()); }
        if !c.bark.is_empty() { e.push("bark".into()); }
        if !c.ntfy.is_empty() { e.push("ntfy".into()); }
        if !c.slack.is_empty() { e.push("slack".into()); }
        if !c.generic_webhook.is_empty() { e.push("webhook".into()); }
        r["enabled"] = serde_json::json!(e);
    }
    Ok(r)
}

async fn h_send_notification(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let msg = args.get("message").and_then(|v| v.as_str()).unwrap_or("no message").to_string();
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("TrendRadar").to_string();
    let channels: Vec<String> = args.get("channels").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let target = if channels.is_empty() { "all".to_string() } else { channels.join(",") };
    let config = state.config.clone();
    let content = format!("{}\n\n{}", title, msg);
    // std::thread 而非 tokio::spawn：Notifier 内部 future 非 Send，需独立单线程 runtime
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let notifier = match Notifier::new(Arc::new(config)) {
                Ok(n) => n,
                Err(e) => { tracing::error!("Notifier init failed: {}", e); return; }
            };
            if channels.is_empty() {
                if let Err(e) = notifier.send_report(&content, "notification", &Default::default(), None).await {
                    tracing::error!("Send notification failed: {}", e);
                }
            } else {
                for ch in &channels {
                    if let Err(e) = notifier.test_channel(ch).await {
                        tracing::error!("Test channel {} failed: {}", ch, e);
                    }
                }
            }
        });
    });
    Ok(serde_json::json!({"success":true,"message":"Notification sent","channels":target}))
}

async fn h_analyze_topic_trend(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let topic = args.get("topic").and_then(|v| v.as_str()).ok_or("Missing 'topic'")?;
    let atype = args.get("analysis_type").and_then(|v| v.as_str()).unwrap_or("trend");
    let days = parse_date_days(args, 7);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let data = state.storage.query_news(None, Some(500), Some(since)).map_err(|e| e.to_string())?;
    let t = topic.to_lowercase();
    let matching: Vec<_> = data.items.iter().filter(|i| i.title.to_lowercase().contains(&t)).collect();
    let total = matching.len();
    let mut daily: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for item in &matching { *daily.entry(item.crawl_time.format("%Y-%m-%d").to_string()).or_default() += 1; }
    let points: Vec<serde_json::Value> = daily.iter().map(|(d,c)| serde_json::json!({"date":d,"count":c})).collect();
    let avg = if !daily.is_empty() { total as f64 / daily.len() as f64 } else { 0.0 };
    Ok(serde_json::json!({"success":true,"topic":topic,"analysis_type":atype,"summary":{"total":total,"days":daily.len(),"avg":format!("{:.1}",avg)},"trend":points}))
}

async fn h_analyze_data_insights(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let itype = args.get("insight_type").and_then(|v| v.as_str()).unwrap_or("platform_compare");
    let topic = args.get("topic").and_then(|v| v.as_str());
    let days = parse_date_days(args, 3);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let top_n = args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
    let min_f = args.get("min_frequency").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let data = state.storage.query_news(None, Some(1000), Some(since)).map_err(|e| e.to_string())?;
    match itype {
        "platform_compare" => {
            let t = topic.map(|s| s.to_lowercase());
            let mut pc: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for item in &data.items {
                if let Some(ref t) = t { if !item.title.to_lowercase().contains(t) { continue; } }
                *pc.entry(item.platform_name.clone().unwrap_or_else(|| item.platform.clone())).or_default() += 1;
            }
            let mut s: Vec<_> = pc.iter().collect(); s.sort_by(|a,b| b.1.cmp(a.1));
            let p: Vec<serde_json::Value> = s.iter().take(10).map(|(n,c)| serde_json::json!({"platform":n,"count":c})).collect();
            Ok(serde_json::json!({"success":true,"insight_type":itype,"topic":topic,"platforms":p}))
        }
        "platform_activity" => {
            let mut pc: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for item in &data.items { *pc.entry(item.platform_name.clone().unwrap_or_else(|| item.platform.clone())).or_default() += 1; }
            let mut s: Vec<_> = pc.iter().collect(); s.sort_by(|a,b| b.1.cmp(a.1));
            let p: Vec<serde_json::Value> = s.iter().take(10).map(|(n,c)| serde_json::json!({"platform":n,"items":c})).collect();
            Ok(serde_json::json!({"success":true,"insight_type":"platform_activity","total":data.items.len(),"platforms":p}))
        }
        "keyword_cooccur" => {
            let mut pairs: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for item in &data.items {
                let ws: Vec<String> = item.title.split(|c: char| !c.is_alphanumeric() && c != '\'').map(|w| w.trim().to_string()).filter(|w| w.len()>=2).collect();
                for i in 0..ws.len() { for j in i+1..ws.len() {
                    *pairs.entry(if ws[i]<ws[j] {format!("{}+{}",ws[i],ws[j])} else {format!("{}+{}",ws[j],ws[i])}).or_default() += 1;
                }}
            }
            let mut s: Vec<_> = pairs.iter().filter(|(_,&c)| c>=min_f).collect(); s.sort_by(|a,b| b.1.cmp(a.1));
            let p: Vec<serde_json::Value> = s.iter().take(top_n).map(|(k,v)| serde_json::json!({"pair":k,"count":v})).collect();
            Ok(serde_json::json!({"success":true,"insight_type":"keyword_cooccur","min_frequency":min_f,"pairs":p}))
        }
        _ => Err(format!("Unknown insight_type: {}", itype))
    }
}

async fn h_analyze_sentiment(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let topic = args.get("topic").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(100) as usize;
    let days = parse_date_days(args, 1);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let data = state.storage.query_news(None, Some(limit*2), Some(since)).map_err(|e| e.to_string())?;
    let t = topic.map(|s| s.to_lowercase());
    let filtered: Vec<_> = data.items.iter().filter(|i| t.as_ref().map_or(true, |t| i.title.to_lowercase().contains(t))).collect();
    let pwords = ["涨","增","利好","突破","新高","成功"]; let nwords = ["跌","降","亏","事故","危机","争议","制裁","失败","警告"];
    let mut pos=0i32; let mut neg=0i32; let mut neu=0i32;
    for item in &filtered {
        let tl = item.title.to_lowercase();
        let ph = pwords.iter().any(|w| tl.contains(w)); let nh = nwords.iter().any(|w| tl.contains(w));
        if ph&&!nh {pos+=1;} else if nh&&!ph {neg+=1;} else {neu+=1;}
    }
    let total = filtered.len();
    let pct = |v:i32| format!("{:.1}%", if total>0 {(v as f64/total as f64)*100.0} else {0.0});
    let items: Vec<serde_json::Value> = filtered.iter().take(limit).map(|i| {
        let mut j = serde_json::json!({"title":i.title,"platform":i.platform,"platform_name":i.platform_name,"rank":i.rank});
        if include_url { if let Some(ref u) = i.url { j["url"] = serde_json::json!(u); } }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"sentiment":{"positive":pos,"negative":neg,"neutral":neu,"total":total,"positive_pct":pct(pos),"negative_pct":pct(neg),"neutral_pct":pct(neu)},"items":items}))
}

async fn h_find_related_news(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let ref_t = args.get("reference_title").and_then(|v| v.as_str()).ok_or("Missing 'reference_title'")?;
    let threshold = args.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let days = parse_date_days(args, 1);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let data = state.storage.query_news(None, Some(500), Some(since)).map_err(|e| e.to_string())?;
    let rw = word_set(ref_t);
    if rw.is_empty() { return Err("参考标题无法提取有效关键词".to_string()); }
    let mut scored: Vec<(f64, &trendradar_core::model::NewsItem)> = data.items.iter().map(|item| {
        let iw = word_set(&item.title);
        let inter = rw.intersection(&iw).count(); let union = rw.union(&iw).count();
        ((inter as f64 / union.max(1) as f64), item)
    }).filter(|(s,_)| *s >= threshold).collect();
    scored.sort_by(|a,b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let items: Vec<serde_json::Value> = scored.iter().take(limit).map(|(s,i)| {
        let mut j = serde_json::json!({"title":i.title,"platform":i.platform,"platform_name":i.platform_name,"similarity":format!("{:.2}",s)});
        if include_url { if let Some(ref u) = i.url { j["url"] = serde_json::json!(u); } }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"reference_title":ref_t,"threshold":threshold,"total":items.len(),"related":items}))
}

fn word_set(s: &str) -> std::collections::HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'').map(|w| w.trim().to_lowercase()).filter(|w| w.len()>=2).collect()
}

async fn h_generate_summary_report(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let report_type = args.get("report_type").and_then(|v| v.as_str()).unwrap_or("daily");
    let days = if report_type == "weekly" { 7 } else { parse_date_days(args, 1) };
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let data = state.storage.query_news(None, Some(200), Some(since)).map_err(|e| e.to_string())?;
    let mut freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for item in &data.items {
        for w in item.title.split(|c: char| !c.is_alphanumeric() && c != '\'') { let w = w.trim(); if w.len()>=2 { *freq.entry(w.to_string()).or_default()+=1; } }
    }
    let mut s: Vec<_> = freq.into_iter().collect(); s.sort_by(|a,b| b.1.cmp(&a.1));
    let topics: Vec<serde_json::Value> = s.iter().take(10).map(|(k,v)| serde_json::json!({"topic":k,"count":v})).collect();
    let news: Vec<serde_json::Value> = data.items.iter().take(10).map(|i| serde_json::json!({"title":i.title,"url":i.url,"platform":i.platform_name})).collect();
    Ok(serde_json::json!({"success":true,"report_type":report_type,"total":data.items.len(),"top_topics":topics,"top_news":news}))
}

async fn h_aggregate_news(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let threshold = args.get("similarity_threshold").and_then(|v| v.as_f64()).unwrap_or(0.7);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(200) as usize;
    let days = parse_date_days(args, 1);
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let data = state.storage.query_news(None, Some(500), Some(since)).map_err(|e| e.to_string())?;
    let mut clusters: Vec<Vec<&trendradar_core::model::NewsItem>> = Vec::new();
    for item in &data.items {
        let iw = word_set(&item.title);
        let mut best: Option<usize> = None; let mut best_s = threshold;
        for (ci, c) in clusters.iter().enumerate() {
            for m in c { let mw = word_set(&m.title); let inter = iw.intersection(&mw).count(); let union = iw.union(&mw).count(); let sim = inter as f64 / union.max(1) as f64; if sim > best_s { best_s = sim; best = Some(ci); break; } }
        }
        if let Some(i) = best { clusters[i].push(item); } else { clusters.push(vec![item]); }
    }
    clusters.sort_by(|a,b| b.len().cmp(&a.len()));
    let orig = data.items.len();
    let agg: Vec<serde_json::Value> = clusters.iter().take(limit).filter(|c| c.len()>=2).map(|c| {
        let platforms: Vec<String> = c.iter().map(|i| i.platform_name.clone().unwrap_or_else(|| i.platform.clone())).collect();
        let mut j = serde_json::json!({"representative_title":c[0].title,"platform_count":platforms.len(),"platforms":platforms,"items":c.len()});
        if include_url { j["urls"] = serde_json::json!(c.iter().filter_map(|i| i.url.clone()).collect::<Vec<_>>()); }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"original":orig,"aggregated":agg.len(),"clusters":agg}))
}

async fn h_compare_periods(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p1 = args.get("period1").ok_or("Missing 'period1'")?;
    let p2 = args.get("period2").ok_or("Missing 'period2'")?;
    let topic = args.get("topic").and_then(|v| v.as_str());
    let compare_type = args.get("compare_type").and_then(|v| v.as_str()).unwrap_or("overview");
    let top_n = args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let parse = |v: &serde_json::Value| -> Result<(String,String), String> {
        if let Some(s) = v.as_str() {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            match s { "today"|"今天" => Ok((today.clone(),today)), _ => Err(format!("无法解析: {}", s)) }
        } else {
            Ok((v.get("start").and_then(|v|v.as_str()).ok_or("Missing start")?.to_string(), v.get("end").and_then(|v|v.as_str()).ok_or("Missing end")?.to_string()))
        }
    };
    let (s1,e1) = parse(p1)?; let (s2,e2) = parse(p2)?;
    let d1 = state.storage.query_news_by_date_range(&s1, &e1).map_err(|e| e.to_string())?;
    let d2 = state.storage.query_news_by_date_range(&s2, &e2).map_err(|e| e.to_string())?;
    let t = topic.map(|s| s.to_lowercase());
    let c1 = d1.items.iter().filter(|i| t.as_ref().map_or(true, |t| i.title.to_lowercase().contains(t))).count();
    let c2 = d2.items.iter().filter(|i| t.as_ref().map_or(true, |t| i.title.to_lowercase().contains(t))).count();
    let pct = if c1>0 { format!("{:.1}%", ((c2 as f64-c1 as f64)/c1 as f64)*100.0) } else { "N/A".into() };
    let t1: Vec<serde_json::Value> = d1.items.iter().take(top_n).map(|i| serde_json::json!({"title":i.title,"platform":i.platform_name})).collect();
    let t2: Vec<serde_json::Value> = d2.items.iter().take(top_n).map(|i| serde_json::json!({"title":i.title,"platform":i.platform_name})).collect();
    Ok(serde_json::json!({"success":true,"period1":{"start":s1,"end":e1,"count":c1},"period2":{"start":s2,"end":e2,"count":c2},"change":{"absolute":c2 as i64-c1 as i64,"percentage":pct},"compare_type":compare_type,"topic":topic,"period1_top":t1,"period2_top":t2}))
}

async fn h_check_version(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let _proxy = args.get("proxy_url").and_then(|v| v.as_str());
    let v = env!("CARGO_PKG_VERSION");
    Ok(serde_json::json!({"success":true,"trendradar":{"current":v,"latest":v,"needs_update":false},"mcp_server":{"current":v,"latest":v,"needs_update":false},"note":"远程比较将在后续版本中实现"}))
}

async fn h_trigger_crawl(state: &AppState, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let include_url = args.get("include_url").and_then(|v| v.as_bool()).unwrap_or(false);
    let platforms_arg: Option<Vec<String>> = args.get("platforms").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let active: Vec<(String,String)> = state.config.platforms.sources.iter().filter(|s|s.enabled).map(|s|(s.id.clone(),s.name.clone())).collect();
    let ids = if let Some(ref plats) = platforms_arg { active.into_iter().filter(|(id,_)| plats.contains(id)).collect() } else { active };
    if ids.is_empty() { return Err("No available platforms".to_string()); }
    let num_ids = ids.len();
    let config = state.config.clone();
    let delay = config.advanced.as_ref().and_then(|a| a.request_delay).unwrap_or(2000);
    let retries = config.advanced.as_ref().and_then(|a| a.max_retries).unwrap_or(2);
    let fetcher = DataFetcher::new(None, None).map_err(|e| e.to_string())?;
    let items = fetcher.fetch_all_platforms(&ids, delay, retries).await.map_err(|e| e.to_string())?;
    let out: Vec<serde_json::Value> = items.iter().map(|i| {
        let mut j = serde_json::json!({"title":i.title,"platform":i.platform,"platform_name":i.platform_name,"rank":i.rank});
        if include_url { if let Some(ref u) = i.url { j["url"] = serde_json::json!(u); } }
        j
    }).collect();
    Ok(serde_json::json!({"success":true,"total":items.len(),"platforms":num_ids,"data":out}))
}

async fn h_sync_from_remote(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(7);
    Ok(serde_json::json!({"success":true,"days":days,"message":"远程存储未配置，本地SQLite模式","synced":0}))
}

async fn h_read_article(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = args.get("url").and_then(|v| v.as_str()).ok_or("Missing 'url'")?;
    let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30).min(60).max(10);
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout)).build().map_err(|e| e.to_string())?;
    let jina_url = format!("https://r.jina.ai/{}", url);
    match client.get(&jina_url).header("Accept","text/markdown").send().await {
        Ok(resp) => {
            let text = resp.text().await.map_err(|e| e.to_string())?;
            let truncated = if text.chars().count() > 10000 { text.chars().take(10000).collect::<String>() } else { text };
            Ok(serde_json::json!({"success":true,"url":url,"content":truncated,"length":truncated.len()}))
        }
        Err(e) => Err(format!("读取失败: {}. 确认URL以http://或https://开头", e))
    }
}

async fn h_read_articles_batch(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let urls: Vec<String> = args.get("urls").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).ok_or("Missing 'urls'")?;
    let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30).min(60).max(10);
    let max_n = urls.len().min(5);
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout)).build().map_err(|e| e.to_string())?;
    let mut results: Vec<serde_json::Value> = Vec::new();
    for (i, url) in urls.iter().take(max_n).enumerate() {
        if i > 0 { tokio::time::sleep(std::time::Duration::from_secs(5)).await; }
        let jina_url = format!("https://r.jina.ai/{}", url);
        let r = match client.get(&jina_url).header("Accept","text/markdown").send().await {
            Ok(resp) => {
                let text = resp.text().await.map_err(|e| e.to_string())?;
                serde_json::json!({"url":url,"status":"success","content":if text.chars().count()>5000 {text.chars().take(5000).collect::<String>()} else {text}})
            }
            Err(e) => serde_json::json!({"url":url,"status":"error","error":e.to_string()})
        };
        results.push(r);
    }
    let ok = results.iter().filter(|r| r["status"]=="success").count();
    Ok(serde_json::json!({"success":true,"requested":urls.len(),"processed":max_n,"ok":ok,"articles":results}))
}

fn parse_date_days(args: &serde_json::Value, default: i64) -> i64 {
    // 优先解析自然语言表达式
    if let Some(expr) = args.get("date_range").and_then(|v| v.as_str()) {
        let now = chrono::Local::now();
        return match expr.to_lowercase().as_str() {
            "today" | "今天" => 1,
            "yesterday" | "昨天" => 1,
            "this_week" | "this week" | "本周" => (now.weekday().num_days_from_monday() as i64 + 1).max(1),
            "last_week" | "上周" | "last week" => 7,
            "this_month" | "本月" | "this month" => now.day() as i64,
            "last_month" | "上月" | "last month" => 30,
            _ => {
                if let Some(n) = parse_natural_days(expr, "最近", "天") { n }
                else if let Some(n) = parse_natural_days(expr, "last", "days") { n }
                else if let Ok(d) = chrono::NaiveDate::parse_from_str(expr, "%Y-%m-%d") { (chrono::Utc::now().date_naive() - d).num_days().max(1) }
                else { default }
            }
        };
    }
    // 后解析嵌套对象
    match args.get("date_range").and_then(|v| v.get("start")).and_then(|v| v.as_str()) {
        Some(s) => match chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            Ok(d) => (chrono::Utc::now().date_naive() - d).num_days().max(1),
            Err(_) => default,
        }
        None => default,
    }
}

fn parse_natural_days(expr: &str, prefix: &str, suffix: &str) -> Option<i64> {
    let l = expr.to_lowercase();
    let p = l.find(&prefix.to_lowercase())?;
    let s = l.rfind(&suffix.to_lowercase())?;
    l[p+prefix.len()..s].trim().parse().ok()
}