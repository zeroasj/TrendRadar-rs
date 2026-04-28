pub mod config;
pub mod model;
pub mod storage;
pub mod crawler;
pub mod matcher;
pub mod ai;
pub mod scheduler;
pub mod timeline;
pub mod report;
pub mod notify;
pub mod error;
pub mod templates;

pub use config::AppConfig;
pub use model::{NewsItem, NewsData, RssItem, RssData, Platform, ReportMode, NotificationChannel};
pub use error::{Result, TrendRadarError};
pub use templates::ReportTemplate;
