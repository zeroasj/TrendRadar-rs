use crate::error::{Result, TrendRadarError};
use crate::model::{NewsItem, NewsData, RssItem, RssData};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SCHEMA_SQL: &str = include_str!("schema.sql");

#[derive(Clone, Debug)]
pub enum StorageBackend {
    Local { db_path: PathBuf },
    Remote { s3: S3Config, local_cache: PathBuf },
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StorageManager {
    backend: StorageBackend,
    conn: Arc<Mutex<Option<Connection>>>,
}

impl StorageManager {
    pub fn new(backend: StorageBackend) -> Result<Self> {
        let mgr = StorageManager {
            backend,
            conn: Arc::new(Mutex::new(None)),
        };
        mgr.init_schema()?;
        Ok(mgr)
    }

    fn db_path(&self) -> &Path {
        match &self.backend {
            StorageBackend::Local { db_path } => db_path.as_path(),
            StorageBackend::Remote { local_cache, .. } => local_cache.as_path(),
        }
    }

    fn connect(&self) -> Result<std::sync::MutexGuard<'_, Option<Connection>>> {
        let mut guard = self.conn.lock().map_err(|e| TrendRadarError::Internal(e.to_string()))?;
        if guard.is_none() {
            let db_path = self.db_path();
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| TrendRadarError::Storage(format!("create db dir: {}", e)))?;
            }
            let conn = Connection::open(db_path)
                .map_err(|e| TrendRadarError::Storage(format!("open db: {}", e)))?;
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=NORMAL;
                 PRAGMA foreign_keys=ON;"
            ).or_else(|_| {
                conn.execute_batch(
                    "PRAGMA journal_mode=DELETE;
                     PRAGMA synchronous=NORMAL;
                     PRAGMA foreign_keys=ON;"
                )
            }).map_err(|e| TrendRadarError::Storage(format!("pragma: {}", e)))?;
            *guard = Some(conn);
        }
        Ok(guard)
    }

    pub fn init_schema(&self) -> Result<()> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        conn.execute_batch(SCHEMA_SQL)
            .map_err(|e| TrendRadarError::Storage(format!("init schema: {}", e)))?;
        Ok(())
    }

    fn map_news_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NewsItem> {
        let url: Option<String> = row.get(0).unwrap_or(None);
        let title: String = row.get(1)?;
        let platform: String = row.get(2)?;
        let platform_name: Option<String> = row.get(3).unwrap_or(None);
        let rank: Option<i32> = row.get(4).unwrap_or(None);
        let hot_score: Option<f64> = row.get(5).unwrap_or(None);
        let summary: Option<String> = row.get(6).unwrap_or(None);
        let author: Option<String> = row.get(7).unwrap_or(None);
        let publish_time_str: Option<String> = row.get(8).unwrap_or(None);
        let crawl_time_str: String = row.get(9)?;
        let category: Option<String> = row.get(10).unwrap_or(None);
        let is_new_int: i32 = row.get(11).unwrap_or(0);

        let publish_time = publish_time_str
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok())
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));

        let crawl_time = chrono::NaiveDateTime::parse_from_str(&crawl_time_str, "%Y-%m-%d %H:%M:%S")
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(NewsItem {
            url, title, platform, platform_name, rank, hot_score,
            summary, author, publish_time, crawl_time, category,
            keywords: vec![], is_new: Some(is_new_int == 1),
            rank_change: None, title_changed: None, appearance_count: 0, id: None,
        })
    }
}

impl StorageManager {
    pub fn ensure_platform(&self, platform_id: &str, name: &str, display_name: Option<&str>) -> Result<String> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO platforms (id, name, display_name) VALUES (?1, ?2, ?3)",
            params![platform_id, name, display_name],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(platform_id.to_string())
    }

    pub fn upsert_news_items(&self, platform_id: &str, platform_name: &str, items: &[NewsItem]) -> Result<(usize, usize)> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        conn.execute_batch("BEGIN TRANSACTION")
            .map_err(|e| TrendRadarError::Storage(format!("begin tx: {}", e)))?;

        let mut new_count = 0;
        let mut updated_count = 0;

        for item in items {
            let url = item.url.as_deref().unwrap_or("");
            let summary = item.summary.as_deref().unwrap_or("");
            let author = item.author.as_deref().unwrap_or("");
            let publish_time = item.publish_time.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string());
            let category = item.category.as_deref().unwrap_or("");

            conn.execute(
                "INSERT OR IGNORE INTO platforms (id, name, display_name) VALUES (?1, ?2, ?3)",
                params![platform_id, platform_name, platform_name],
            ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

            let existing: Option<(i64, String, Option<i32>, Option<f64>)> = if !url.is_empty() {
                conn.query_row(
                    "SELECT id, title, rank, hot_score FROM news_items WHERE url = ?1 AND platform_id = ?2",
                    params![url, platform_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                ).optional().map_err(|e| TrendRadarError::Storage(e.to_string()))?
            } else {
                conn.query_row(
                    "SELECT id, title, rank, hot_score FROM news_items WHERE title = ?1 AND platform_id = ?2",
                    params![&item.title, platform_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                ).optional().map_err(|e| TrendRadarError::Storage(e.to_string()))?
            };

            if let Some((news_id, old_title, old_rank, old_score)) = existing {
                updated_count += 1;

                if old_title != item.title {
                    conn.execute(
                        "INSERT INTO title_changes (news_id, old_title, new_title, change_time) VALUES (?1, ?2, ?3, ?4)",
                        params![news_id, &old_title, &item.title, &now],
                    ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
                }

                conn.execute(
                    "UPDATE news_items SET title=?1, rank=?2, hot_score=?3, summary=?4, author=?5, category=?6, crawl_time=?7, is_new=0 WHERE id=?8",
                    params![&item.title, item.rank, item.hot_score, summary, author, category, &now, news_id],
                ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

                if old_rank != item.rank || old_score != item.hot_score {
                    conn.execute(
                        "INSERT INTO rank_history (news_id, rank, hot_score, record_time) VALUES (?1, ?2, ?3, ?4)",
                        params![news_id, item.rank, item.hot_score, &now],
                    ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
                }
            } else {
                new_count += 1;
                conn.execute(
                    "INSERT INTO news_items (url, title, platform_id, rank, hot_score, summary, author, publish_time, crawl_time, category) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![url, &item.title, platform_id, item.rank, item.hot_score, summary, author, publish_time, &now, category],
                ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

                let news_id = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO rank_history (news_id, rank, hot_score, record_time) VALUES (?1, ?2, ?3, ?4)",
                    params![news_id, item.rank, item.hot_score, &now],
                ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            }
        }

        conn.execute_batch("COMMIT")
            .map_err(|e| TrendRadarError::Storage(format!("commit tx: {}", e)))?;

        Ok((new_count, updated_count))
    }

    pub fn mark_off_list_items(&self, platform_id: &str, active_urls: &[&str]) -> Result<usize> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if active_urls.is_empty() {
            conn.execute(
                "UPDATE news_items SET rank = 0, crawl_time = ?1 WHERE platform_id = ?2 AND rank > 0",
                params![&now, platform_id],
            ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            Ok(conn.changes() as usize)
        } else {
            let placeholders = active_urls.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "UPDATE news_items SET rank = 0, crawl_time = ?1 WHERE platform_id = ?2 AND rank > 0 AND url NOT IN ({})",
                placeholders
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
                Box::new(now),
                Box::new(platform_id.to_string()),
            ];
            for url in active_urls {
                param_values.push(Box::new(url.to_string()));
            }
            let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
            stmt.execute(params_ref.as_slice()).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            Ok(conn.changes() as usize)
        }
    }

    pub fn insert_crawl_record(&self, platform_id: &str, items_count: usize, new_count: usize, duration_ms: u64, success: bool, error: Option<&str>) -> Result<()> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO crawl_records (platform_id, crawl_time, items_count, new_items_count, duration_ms, success, error_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![platform_id, &now, items_count as i64, new_count as i64, duration_ms as i64, success as i64, error],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn query_news(&self, platform: Option<&str>, limit: Option<usize>, since: Option<DateTime<Utc>>) -> Result<NewsData> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();

        let mut sql = String::from(
            "SELECT n.url, n.title, p.id as platform, p.display_name, n.rank, n.hot_score, n.summary, n.author, n.publish_time, n.crawl_time, n.category, n.is_new \
             FROM news_items n JOIN platforms p ON n.platform_id = p.id WHERE 1=1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 1;

        if let Some(plat) = platform {
            sql.push_str(&format!(" AND n.platform_id = ?{}", param_idx));
            param_values.push(Box::new(plat.to_string()));
            param_idx += 1;
        }
        if let Some(since_time) = since {
            sql.push_str(&format!(" AND n.crawl_time >= ?{}", param_idx));
            param_values.push(Box::new(since_time.format("%Y-%m-%d %H:%M:%S").to_string()));
            param_idx += 1;
        }
        sql.push_str(" ORDER BY n.crawl_time DESC, n.rank ASC");

        if let Some(lim) = limit {
            sql.push_str(&format!(" LIMIT ?{}", param_idx));
            param_values.push(Box::new(lim as i64));
        }

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        let items = stmt.query_map(params_ref.as_slice(), |row| {
            Self::map_news_row(row)
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

        let total = items.len();
        Ok(NewsData { items, total, crawl_time: Utc::now() })
    }

    pub fn query_news_by_date(&self, date: &str) -> Result<NewsData> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT n.url, n.title, p.id as platform, p.display_name, n.rank, n.hot_score, n.summary, n.author, n.publish_time, n.crawl_time, n.category, n.is_new \
             FROM news_items n JOIN platforms p ON n.platform_id = p.id WHERE date(n.crawl_time) = ?1 ORDER BY n.rank ASC"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let items = stmt.query_map(params![date], |row| {
            Self::map_news_row(row)
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

        let total = items.len();
        Ok(NewsData { items, total, crawl_time: Utc::now() })
    }

    pub fn query_news_by_date_range(&self, start: &str, end: &str) -> Result<NewsData> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let sql = "SELECT n.url, n.title, p.id as platform, p.display_name, n.rank, n.hot_score, n.summary, n.author, n.publish_time, n.crawl_time, n.category, n.is_new \
                   FROM news_items n JOIN platforms p ON n.platform_id = p.id \
                   WHERE date(n.crawl_time) >= ?1 AND date(n.crawl_time) <= ?2 ORDER BY n.crawl_time DESC, n.rank ASC";
        let mut stmt = conn.prepare(sql).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        let items = stmt.query_map(params![start, end], |row| {
            Self::map_news_row(row)
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
        let total = items.len();
        Ok(NewsData { items, total, crawl_time: Utc::now() })
    }

    pub fn search_news_by_title(&self, query: &str, limit: usize, since: Option<DateTime<Utc>>) -> Result<NewsData> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut sql = String::from(
            "SELECT n.url, n.title, p.id as platform, p.display_name, n.rank, n.hot_score, n.summary, n.author, n.publish_time, n.crawl_time, n.category, n.is_new \
             FROM news_items n JOIN platforms p ON n.platform_id = p.id WHERE n.title LIKE ?1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let escaped = query.replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{}%", escaped);
        param_values.push(Box::new(pattern));
        if let Some(since_time) = since {
            sql.push_str(" AND n.crawl_time >= ?2");
            param_values.push(Box::new(since_time.format("%Y-%m-%d %H:%M:%S").to_string()));
        }
        sql.push_str(" ORDER BY n.crawl_time DESC, n.rank ASC");
        sql.push_str(&format!(" LIMIT {}", limit));
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        let items = stmt.query_map(params_ref.as_slice(), |row| {
            Self::map_news_row(row)
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();
        let total = items.len();
        Ok(NewsData { items, total, crawl_time: Utc::now() })
    }

    pub fn list_available_dates(&self) -> Result<Vec<String>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date(crawl_time) FROM news_items ORDER BY date(crawl_time) DESC"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        let dates = stmt.query_map(params![], |row| {
            Ok(row.get::<_, String>(0)?)
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
        Ok(dates)
    }

    pub fn get_rank_history(&self, news_id: i64) -> Result<Vec<(i32, Option<f64>, String)>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT rank, hot_score, record_time FROM rank_history WHERE news_id = ?1 ORDER BY record_time ASC"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let history = stmt.query_map(params![news_id], |row| {
            let rank: i32 = row.get(0)?;
            let hot_score: Option<f64> = row.get(1).unwrap_or(None);
            let record_time: String = row.get(2)?;
            Ok((rank, hot_score, record_time))
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(history)
    }

    pub fn get_title_changes(&self, news_id: i64) -> Result<Vec<(String, String, String)>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT old_title, new_title, change_time FROM title_changes WHERE news_id = ?1 ORDER BY change_time ASC"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let changes = stmt.query_map(params![news_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(changes)
    }
}

impl StorageManager {
    pub fn ensure_rss_feed(&self, name: &str, url: &str) -> Result<i64> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO rss_feeds (name, url) VALUES (?1, ?2)",
            params![name, url],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        let id: i64 = conn.query_row(
            "SELECT id FROM rss_feeds WHERE url = ?1",
            params![url],
            |row| row.get(0),
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(id)
    }

    pub fn upsert_rss_items(&self, feed_name: &str, feed_url: &str, items: &[RssItem]) -> Result<(usize, usize)> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        conn.execute_batch("BEGIN TRANSACTION")
            .map_err(|e| TrendRadarError::Storage(format!("begin tx: {}", e)))?;

        conn.execute(
            "INSERT OR IGNORE INTO rss_feeds (name, url) VALUES (?1, ?2)",
            params![feed_name, feed_url],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let feed_id: i64 = conn.query_row(
            "SELECT id FROM rss_feeds WHERE url = ?1",
            params![feed_url],
            |row| row.get(0),
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let mut new_count = 0;
        let mut updated_count = 0;

        for item in items {
            let link = item.link.as_deref().unwrap_or("");
            let description = item.description.as_deref().unwrap_or("");
            let author = item.author.as_deref().unwrap_or("");
            let publish_time = item.publish_time.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string());
            let guid = item.guid.as_deref().unwrap_or("");

            let existing: Option<i64> = conn.query_row(
                "SELECT id FROM rss_items WHERE feed_id = ?1 AND guid = ?2",
                params![feed_id, guid],
                |row| row.get(0),
            ).optional().map_err(|e| TrendRadarError::Storage(e.to_string()))?;

            if let Some(_) = existing {
                updated_count += 1;
                conn.execute(
                    "UPDATE rss_items SET title=?1, link=?2, description=?3, author=?4, publish_time=?5, crawl_time=?6, is_new=0 WHERE feed_id=?7 AND guid=?8",
                    params![&item.title, link, description, author, publish_time, &now, feed_id, guid],
                ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            } else {
                new_count += 1;
                conn.execute(
                    "INSERT INTO rss_items (feed_id, title, link, description, author, publish_time, crawl_time, guid) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![feed_id, &item.title, link, description, author, publish_time, &now, guid],
                ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            }
        }

        conn.execute_batch("COMMIT")
            .map_err(|e| TrendRadarError::Storage(format!("commit tx: {}", e)))?;

        Ok((new_count, updated_count))
    }

    pub fn query_rss(&self, feed: Option<&str>, limit: Option<usize>, since: Option<DateTime<Utc>>) -> Result<RssData> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();

        let mut sql = String::from(
            "SELECT r.title, r.link, r.description, r.author, r.publish_time, r.crawl_time, f.name, r.guid \
             FROM rss_items r JOIN rss_feeds f ON r.feed_id = f.id WHERE 1=1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 1;

        if let Some(f) = feed {
            sql.push_str(&format!(" AND f.name = ?{}", param_idx));
            param_values.push(Box::new(f.to_string()));
            param_idx += 1;
        }
        if let Some(since_time) = since {
            sql.push_str(&format!(" AND r.crawl_time >= ?{}", param_idx));
            param_values.push(Box::new(since_time.format("%Y-%m-%d %H:%M:%S").to_string()));
            param_idx += 1;
        }
        sql.push_str(" ORDER BY r.crawl_time DESC");

        if let Some(lim) = limit {
            sql.push_str(&format!(" LIMIT ?{}", param_idx));
            param_values.push(Box::new(lim as i64));
        }

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        let items = stmt.query_map(params_ref.as_slice(), |row| {
            let title: String = row.get(0)?;
            let link: Option<String> = row.get(1).unwrap_or(None);
            let description: Option<String> = row.get(2).unwrap_or(None);
            let author: Option<String> = row.get(3).unwrap_or(None);
            let publish_time_str: Option<String> = row.get(4).unwrap_or(None);
            let crawl_time_str: String = row.get(5)?;
            let feed_name: String = row.get(6)?;
            let guid: Option<String> = row.get(7).unwrap_or(None);

            let publish_time = publish_time_str
                .and_then(|s| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok())
                .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
            let crawl_time = chrono::NaiveDateTime::parse_from_str(&crawl_time_str, "%Y-%m-%d %H:%M:%S")
                .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(RssItem {
                title,
                link,
                description,
                author,
                publish_time,
                crawl_time,
                feed_name,
                guid,
                keywords: vec![],
                summary: None,
                feed_id: None,
                title_changed: false,
            })
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

        let total = items.len();
        Ok(RssData { items, total, crawl_time: Utc::now() })
    }

    pub fn get_rss_feed_ids(&self) -> Result<Vec<(i64, String, String)>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, url FROM rss_feeds WHERE enabled = 1"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let feeds = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(feeds)
    }
}

impl StorageManager {
    pub fn upsert_ai_filter_tag(&self, tag: &str) -> Result<()> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT OR IGNORE INTO ai_filter_tags (tag, created_time) VALUES (?1, ?2)",
            params![tag, &now],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn insert_ai_filter_result(&self, news_url: &str, tag: &str, confidence: Option<f64>, reason: Option<&str>) -> Result<()> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO ai_filter_results (news_url, tag, confidence, reason, filter_time) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![news_url, tag, confidence, reason, &now],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn get_ai_filter_results(&self, news_url: &str) -> Result<Vec<(String, Option<f64>, Option<String>)>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tag, confidence, reason FROM ai_filter_results WHERE news_url = ?1 ORDER BY filter_time DESC"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let results = stmt.query_map(params![news_url], |row| {
            let tag: String = row.get(0)?;
            let confidence: Option<f64> = row.get(1).unwrap_or(None);
            let reason: Option<String> = row.get(2).unwrap_or(None);
            Ok((tag, confidence, reason))
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(results)
    }

    pub fn get_all_ai_filter_tags(&self) -> Result<Vec<String>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tag FROM ai_filter_tags ORDER BY created_time"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let tags = stmt.query_map([], |row| {
            row.get(0)
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(tags)
    }

    pub fn is_news_analyzed_by_ai(&self, news_url: &str) -> Result<bool> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM ai_filter_analyzed_news WHERE news_url = ?1",
            params![news_url],
            |row| row.get(0),
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(exists)
    }

    pub fn mark_news_ai_analyzed(&self, news_url: &str) -> Result<()> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT OR IGNORE INTO ai_filter_analyzed_news (news_url, analyzed_time) VALUES (?1, ?2)",
            params![news_url, &now],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn delete_old_ai_filter_results(&self, before_days: i64) -> Result<usize> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let cutoff = (Utc::now() - chrono::Duration::days(before_days))
            .format("%Y-%m-%d %H:%M:%S").to_string();
        let count = conn.execute(
            "DELETE FROM ai_filter_results WHERE filter_time < ?1",
            params![&cutoff],
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
        Ok(count)
    }
}

impl StorageManager {
    pub fn vacuum(&self) -> Result<()> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        conn.execute("VACUUM", [])
            .map_err(|e| TrendRadarError::Storage(format!("vacuum: {}", e)))?;
        Ok(())
    }

    pub fn get_db_size_bytes(&self) -> Result<u64> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let size: i64 = conn.query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        Ok(size as u64)
    }

    pub fn get_table_counts(&self) -> Result<std::collections::HashMap<String, i64>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut counts = std::collections::HashMap::new();

        let tables = vec![
            "platforms", "news_items", "title_changes", "rank_history",
            "crawl_records", "rss_feeds", "rss_items", "ai_filter_tags",
            "ai_filter_results", "ai_filter_analyzed_news",
        ];

        for table in tables {
            let count: i64 = conn.query_row(
                &format!("SELECT COUNT(*) FROM {}", table),
                [],
                |row| row.get(0),
            ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;
            counts.insert(table.to_string(), count);
        }

        Ok(counts)
    }

    pub fn get_platforms(&self) -> Result<Vec<(String, String, Option<String>, bool)>> {
        let guard = self.connect()?;
        let conn = guard.as_ref().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, display_name, enabled FROM platforms ORDER BY id"
        ).map_err(|e| TrendRadarError::Storage(e.to_string()))?;

        let platforms = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2).unwrap_or(None), row.get::<_, i32>(3).unwrap_or(1) == 1))
        }).map_err(|e| TrendRadarError::Storage(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

        Ok(platforms)
    }

    pub fn download_from_s3(&self) -> Result<()> {
        match &self.backend {
            StorageBackend::Local { .. } => Ok(()),
            StorageBackend::Remote { .. } => {
                Ok(())
            }
        }
    }

    pub fn upload_to_s3(&self) -> Result<()> {
        match &self.backend {
            StorageBackend::Local { .. } => Ok(()),
            StorageBackend::Remote { .. } => {
                Ok(())
            }
        }
    }
}
