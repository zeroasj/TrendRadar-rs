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
pub struct McpError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub storage: StorageManager,
    pub config_path: String,
}

impl AppState {
    pub fn new(config: AppConfig, storage: StorageManager, config_path: &str) -> Self {
        AppState { config, storage, config_path: config_path.to_string() }
    }
}
