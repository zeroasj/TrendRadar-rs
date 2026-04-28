-- TrendRadar Schema
CREATE TABLE IF NOT EXISTS platforms (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    display_name TEXT,
    enabled INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS news_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT,
    title TEXT NOT NULL,
    platform_id TEXT NOT NULL,
    rank INTEGER,
    hot_score REAL,
    summary TEXT,
    author TEXT,
    publish_time TEXT,
    crawl_time TEXT NOT NULL,
    category TEXT,
    is_new INTEGER DEFAULT 1,
    is_title_changed INTEGER DEFAULT 0,
    UNIQUE(url, platform_id)
);

CREATE TABLE IF NOT EXISTS rank_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    news_id INTEGER NOT NULL,
    rank INTEGER,
    hot_score REAL,
    record_time TEXT NOT NULL,
    FOREIGN KEY (news_id) REFERENCES news_items(id)
);

CREATE TABLE IF NOT EXISTS title_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    news_id INTEGER NOT NULL,
    old_title TEXT NOT NULL,
    new_title TEXT NOT NULL,
    change_time TEXT NOT NULL,
    FOREIGN KEY (news_id) REFERENCES news_items(id)
);

CREATE TABLE IF NOT EXISTS crawl_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform_id TEXT NOT NULL,
    crawl_time TEXT NOT NULL,
    items_count INTEGER DEFAULT 0,
    new_items_count INTEGER DEFAULT 0,
    duration_ms INTEGER,
    success INTEGER DEFAULT 1,
    error_message TEXT
);

CREATE TABLE IF NOT EXISTS rss_feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    enabled INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS rss_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    link TEXT,
    description TEXT,
    author TEXT,
    publish_time TEXT,
    crawl_time TEXT NOT NULL,
    guid TEXT,
    is_new INTEGER DEFAULT 1,
    FOREIGN KEY (feed_id) REFERENCES rss_feeds(id),
    UNIQUE(feed_id, guid)
);

CREATE TABLE IF NOT EXISTS ai_filter_tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tag TEXT NOT NULL UNIQUE,
    created_time TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_filter_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    news_url TEXT,
    tag TEXT NOT NULL,
    confidence REAL,
    reason TEXT,
    filter_time TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_filter_analyzed_news (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    news_url TEXT NOT NULL UNIQUE,
    analyzed_time TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_news_platform ON news_items(platform_id);
CREATE INDEX IF NOT EXISTS idx_news_crawl_time ON news_items(crawl_time);
CREATE INDEX IF NOT EXISTS idx_news_url_platform ON news_items(url, platform_id);
CREATE INDEX IF NOT EXISTS idx_news_title_platform ON news_items(title, platform_id);
CREATE INDEX IF NOT EXISTS idx_rank_history_news ON rank_history(news_id);
CREATE INDEX IF NOT EXISTS idx_title_changes_news ON title_changes(news_id);
CREATE INDEX IF NOT EXISTS idx_crawl_records_platform ON crawl_records(platform_id);
CREATE INDEX IF NOT EXISTS idx_rss_items_feed ON rss_items(feed_id);
CREATE INDEX IF NOT EXISTS idx_rss_items_crawl_time ON rss_items(crawl_time);
CREATE INDEX IF NOT EXISTS idx_ai_filter_results_time ON ai_filter_results(filter_time);
CREATE INDEX IF NOT EXISTS idx_ai_filter_results_url ON ai_filter_results(news_url);
