use askama::Template;
use chrono::Local;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::fmt::time::LocalTime;
use std::time::Duration;
use trendradar_core::ai::{AiAnalyzer, AiClient, AiFilter, AiTranslator};
use trendradar_core::config::AppConfig;
use trendradar_core::crawler::{DataFetcher, RssFetcher};
use trendradar_core::matcher::KeywordMatcher;
use trendradar_core::model::{NewsItem, ReportMode, RssItem};
use trendradar_core::notify::Notifier;
use trendradar_core::report::{
    generate_markdown_report, generate_notification_content, generate_rss_report,
    NewsDisplay, ReportData, ReportWriter, StatItem,
};
use trendradar_core::scheduler::TimelineScheduler;
use trendradar_core::templates::ReportTemplate;
use trendradar_core::storage::{StorageBackend, StorageManager};
use trendradar_platform::mcp;

#[derive(Parser)]
#[command(
    name = "trendradar",
    version,
    about = "TrendRadar - 全网热点聚合 · AI 深度分析 · 多层筛选 · 多渠推送",
    long_about = "多平台热榜聚合与智能分析系统。支持 11 个热榜平台、RSS 订阅、关键词匹配、AI 智能筛选、9 种通知渠道推送、HTML/RSS/Markdown 报告生成。"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(
        short,
        long,
        default_value = "config/config.yaml",
        help = "配置文件路径"
    )]
    config: String,

    #[arg(long, help = "显示当前调度计划")]
    show_schedule: bool,

    #[arg(long, help = "检查配置并诊断潜在问题")]
    doctor: bool,

    #[arg(short, long, help = "测试通知渠道（需指定渠道名）")]
    test_notification: Option<String>,

    #[arg(
        long,
        default_value = "false",
        help = "仅执行一次后退出（不进入调度循环）"
    )]
    once: bool,

    #[arg(
        short,
        long,
        help = "报告模式: daily, current, incremental（不传则使用 config.yaml 的 report.mode）"
    )]
    mode: Option<String>,
}

#[derive(clap::Subcommand)]
enum Command {
    Run,
    Serve {
        #[arg(short, long, default_value = "0.0.0.0")]
        host: String,
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 在初始化日志之前先读 config 中的时区并设置环境变量
    // 这样 tracing subscriber 和 chrono 都能用正确的时区
    preload_timezone(&cli.config);

    tracing_subscriber::fmt()
        .with_timer(LocalTime::rfc_3339())
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_dir = std::path::Path::new(&cli.config)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    tracing::info!("TrendRadar v{} 启动中...", env!("CARGO_PKG_VERSION"));
    tracing::info!("配置文件: {}", &cli.config);

    load_dotenv();
    if std::env::var("SMTP_PASSWORD").is_ok() || std::env::var("AI_API_KEY").is_ok() {
        tracing::info!(".env 环境变量覆盖已生效");
    }

    let mut config = match AppConfig::load(&cli.config) {
        Ok(c) => {
            tracing::info!("配置加载成功");
            c
        }
        Err(e) => {
            tracing::error!("配置加载失败: {}", e);
            return Err(anyhow::anyhow!("配置加载失败: {}", e));
        }
    };

    // 未配置调度时间段但开启了调度时，注入默认 period（每小时触发一次）
    let schedule_empty = config.schedule.as_ref().map(|s| s.periods.is_empty()).unwrap_or(true);
    let schedule_enabled = config.schedule.as_ref().and_then(|s| s.enabled).unwrap_or(false);
    if schedule_empty && schedule_enabled {
        // 如果有 timeline preset 就依赖 timeline.yaml，不再注入默认 period
        let has_timeline_preset = config.schedule.as_ref()
            .and_then(|s| s.preset.as_ref())
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        if !has_timeline_preset {
            use trendradar_core::config::SchedulePeriod;
            config.schedule.get_or_insert_with(Default::default).periods.push(SchedulePeriod {
                id: "default".to_string(),
                name: Some("默认".to_string()),
                enabled: true,
                cron: None,
                run_days: None,
                run_hours: None,
                run_minutes: None,
                collect: Some(true),
                analyze: Some(config.ai_analysis.as_ref().and_then(|a| a.enabled).unwrap_or(false)),
                push: Some(config.notification.as_ref().map(|n| n.enabled.unwrap_or(false)).unwrap_or(false)),
                report_mode: Some("current".to_string()),
                platforms: Vec::new(),
            });
            tracing::info!("未配置调度时间段，使用默认设置（每小时触发一次）");
        }
    }

    let config = Arc::new(config);

    if cli.show_schedule {
        run_doctor(&config);
        return Ok(());
    }

    if cli.doctor {
        run_doctor(&config);
        return Ok(());
    }

    if let Some(channel) = cli.test_notification.as_deref() {
        let notifier = Notifier::new(config.clone())?;
        notifier.test_channel(channel).await?;
        return Ok(());
    }

    let command = cli.command.unwrap_or(Command::Run);

    match command {
        Command::Serve { host, port } => {
            tracing::info!("启动 MCP Server 于 {}:{}", host, port);
            let db_path = config.storage.as_ref()
                .and_then(|s| s.data_dir.as_deref())
                .unwrap_or("data");
            let storage = StorageManager::new(StorageBackend::Local {
                db_path: PathBuf::from(db_path).join("trendradar.db"),
            })?;
            storage.init_schema()?;
            let app_config = (*config).clone();
            mcp::serve(app_config, storage, &host, port).await?;
        }
        Command::Run => {
            let once = cli.once;
            let cli_mode = cli.mode.clone();
            run_pipeline(config, cli_mode, once, config_dir).await?;
        }
    }

    Ok(())
}

fn run_doctor(config: &AppConfig) {
    println!("=== TrendRadar 配置诊断 ===\n");

    println!("[调度计划]");
    if let Some(schedule) = &config.schedule {
        for period in &schedule.periods {
            let status = if period.enabled { "✔ 启用" } else { "✘ 禁用" };
            println!(
                "  {} {} | cron={} | mode={}",
                status,
                period.name.as_deref().unwrap_or(&period.id),
                period.cron.as_deref().unwrap_or("N/A"),
                period.report_mode.as_deref().unwrap_or("default")
            );
        }
    } else {
        println!("  未配置调度计划");
    }

    println!("\n[热榜平台]");
    let platform_names: Vec<String> = config.platforms.sources.iter()
        .filter(|s| s.enabled)
        .map(|s| s.name.clone())
        .collect();
    println!("  启用的平台: {}", platform_names.join(", "));

    println!("\n[RSS 订阅]");
    if let Some(rss) = &config.rss {
        println!("  订阅源: {} 个", rss.feeds.len());
        for feed in &rss.feeds {
            println!(
                "  - {}",
                feed.name.as_deref().unwrap_or(&feed.url)
            );
        }
    } else {
        println!("  未配置 RSS 订阅");
    }

    println!("\n[关键词过滤]");
    if let Some(filter) = &config.filter {
        println!("  匹配关键词: {}", filter.keywords.join(", "));
        if !filter.exclude_keywords.is_empty() {
            println!("  排除关键词: {}", filter.exclude_keywords.join(", "));
        }
    } else {
        println!("  未配置关键词过滤");
    }

    println!("\n[通知渠道]");
    if let Some(notify) = &config.notification {
        if !notify.enabled.unwrap_or(false) {
            println!("  ✘ 通知已禁用");
        } else {
            println!("  ✔ 通知已启用");
            let channels = notify.channels.effective_channels();
            let mut any_configured = false;
            for (name, configured) in &channels {
                if *configured {
                    println!("  ✔ {}", name);
                    any_configured = true;
                }
            }
            if !any_configured {
                println!("  ⚠ 未配置任何推送渠道（必填字段为空）");
            }
        }
    } else {
        println!("  未配置通知模块");
    }

    println!("\n[AI 分析]");
    if let Some(ai) = &config.ai {
        let model = ai.model.as_deref().unwrap_or("未指定");
        println!("  模型: {}", model);
        let api_base = ai.api_base.as_deref().unwrap_or("默认");
        println!("  API: {}", api_base);
        println!("  API Key: {}", if ai.api_key.is_some() { "已配置" } else { "未配置" });
    } else {
        println!("  未配置 AI 分析");
    }

    println!("\n[显示设置]");
    if let Some(display) = &config.display {
        println!("  标题栏: {}", if display.show_title_section { "✔" } else { "✘" });
        println!("  统计概览: {}", if display.show_total_statistics { "✔" } else { "✘" });
        println!("  频率分析: {}", if display.show_frequency_analysis { "✔" } else { "✘" });
        println!("  新增热点: {}", if display.show_new_section { "✔" } else { "✘" });
        println!("  失败数据源: {}", if display.show_failed_sources { "✔" } else { "✘" });
    }

    println!("\n=== 诊断完成 ===");
}

fn load_dotenv() {
    let path = std::path::Path::new(".env");
    if !path.exists() {
        return;
    }
    if let Ok(contents) = std::fs::read_to_string(path) {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(eq) = trimmed.find('=') {
                let key = trimmed[..eq].trim();
                let val = trimmed[eq + 1..].trim();
                let val = val.trim_matches('"').trim_matches('\'');
                if !key.is_empty() {
                    std::env::set_var(key, val);
                }
            }
        }
    }
}

fn preload_timezone(config_path: &str) {
    if let Ok(contents) = std::fs::read_to_string(config_path) {
        if let Ok(root) = serde_yaml::from_str::<serde_yaml::Value>(&contents) {
            if let Some(tz) = root.get("app").and_then(|a| a.get("timezone")).and_then(|v| v.as_str()) {
                std::env::set_var("TZ", tz);
            }
        }
    }
}

fn load_ai_prompt(config: &trendradar_core::config::AiAnalysisSettings, _global_ai: &Option<&trendradar_core::config::AiSettings>, base_dir: &std::path::Path) -> (String, String) {
    let rel_path = config.prompt_file.as_deref().unwrap_or("ai_analysis_prompt.txt");
    let prompt_file = if std::path::Path::new(rel_path).is_absolute() {
        rel_path.to_string()
    } else {
        base_dir.join(rel_path).display().to_string()
    };
    match std::fs::read_to_string(&prompt_file) {
        Ok(content) => {
            let mut system = String::new();
            let mut user = String::new();
            let mut current_section = "";
            for line in content.lines() {
                if line.trim() == "[system]" { current_section = "system"; continue; }
                if line.trim() == "[user]" { current_section = "user"; continue; }
                if !current_section.is_empty() {
                    match current_section {
                        "system" => { system.push_str(line); system.push('\n'); }
                        "user" => { user.push_str(line); user.push('\n'); }
                        _ => {}
                    }
                }
            }
            let system = system.trim().to_string();
            let user = user.trim().to_string();
            if !system.is_empty() && !user.is_empty() {
                tracing::info!("从文件加载 AI 提示词: {}", prompt_file);
                return (system, user);
            }
        }
        Err(e) => tracing::warn!("无法读取 {}: {}, 使用默认提示词", prompt_file, e),
    }
    (
        "你是一个专业的热点新闻分析师，擅长从多平台热榜数据中提炼核心趋势、发现潜在信号、研判舆论走向。".to_string(),
        "请对以下热点新闻进行深度分析。\n\n## 热榜数据\n{news_content}\n\n## RSS 数据\n{rss_content}\n\n## 分析时间\n{query_time}\n\n请从以下维度进行分析：\n1. 核心热点与舆情态势\n2. 舆论风向与争议\n3. 异动与弱信号\n4. RSS 深度洞察\n5. 研判与策略建议\n\n请以 JSON 格式返回分析结果，包含以下字段：core_trends, sentiment_controversy, signals, rss_insights, outlook_strategy".to_string(),
    )
}

async fn run_pipeline(config: Arc<AppConfig>, cli_mode: Option<String>, once: bool, base_dir: std::path::PathBuf) -> anyhow::Result<()> {
    // 切换到配置文件所在目录，确保所有相对路径正确
    std::env::set_current_dir(&base_dir)?;

    // 回退报告模式：CLI > config.yaml report.mode
    let fallback_mode_str = config.report.as_ref()
        .and_then(|r| r.mode.clone())
        .unwrap_or_else(|| "current".to_string());
    let mut mode: ReportMode = cli_mode
        .clone()
        .unwrap_or_else(|| fallback_mode_str.clone())
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let mut mode_label = format!("{:?}", mode).to_lowercase();

    let db_path = config
        .storage
        .as_ref()
        .and_then(|s| s.data_dir.as_deref())
        .map(|d| format!("{}/trendradar.db", d))
        .unwrap_or_else(|| "data/trendradar.db".to_string());
    let db_dir = std::path::Path::new(&db_path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(db_dir)?;

    let storage = StorageManager::new(StorageBackend::Local {
        db_path: PathBuf::from(&db_path),
    })?;
    tracing::info!("SQLite 数据库: {}", db_path);

    let filter_config = config.filter.as_ref();
    let mut keywords: Vec<String> = filter_config
        .map(|f| f.keywords.clone())
        .unwrap_or_default();
    let exclude_keywords: Vec<String> = filter_config
        .map(|f| f.exclude_keywords.clone())
        .unwrap_or_default();

    // 从 frequency_words.txt 补充关键词（与 config.yaml 的 filter.keywords 合并）
    if let Ok(content) = std::fs::read_to_string("frequency_words.txt") {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
                continue;
            }
            let word = trimmed.split(':').next().unwrap_or(trimmed).trim();
            if !word.is_empty() && !keywords.contains(&word.to_string()) {
                keywords.push(word.to_string());
            }
        }
        tracing::info!("从 frequency_words.txt 加载 {} 个关键词", keywords.len());
    }
    let matcher = KeywordMatcher::new(&keywords, &exclude_keywords)?;

    let data_fetcher = DataFetcher::new(None, None)?;

    let rss_fetcher = RssFetcher::from_config(&config)?;

    let ai_client = match config.ai.as_ref() {
        Some(ai_settings) => Some(AiClient::new(ai_settings)?),
        None => None,
    };

    let output_dir = config
        .report
        .as_ref()
        .and_then(|r| r.output_dir.as_deref())
        .unwrap_or("output");

    let notifier = Notifier::new(config.clone())
        .ok()
        .or_else(|| {
            tracing::warn!("通知模块初始化失败，将跳过推送到所有渠道");
            None
        });

    let display_settings = config
        .display
        .as_ref()
        .cloned()
        .unwrap_or_default();

    let scheduler = TimelineScheduler::new(config.clone());

    // timeline.yaml 解析：CLI 未指定 --mode 时，用调度结果覆盖报告模式
    let resolved = scheduler.resolve();
    if cli_mode.is_none() && resolved.report_mode != mode_label {
        tracing::info!("[调度] 报告模式覆盖: {} -> {}", mode_label, resolved.report_mode);
        mode = resolved.report_mode.parse().map_err(|e: String| anyhow::anyhow!(e))?;
        mode_label = format!("{:?}", mode).to_lowercase();
    }
    if !resolved.collect {
        tracing::info!("[调度] 当前时间段不执行数据采集，退出");
        return Ok(());
    }

    let active_sources: Vec<(&str, &str)> = config
        .platforms
        .sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();

    let platform_ids: Vec<(String, String)> = if active_sources.is_empty() {
        DataFetcher::get_platform_ids()
    } else {
        active_sources.into_iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
    };

    let request_interval_ms = config
        .advanced
        .as_ref()
        .and_then(|a| a.request_delay)
        .unwrap_or(2000);

    let data_retention_days: i64 = config
        .advanced
        .as_ref()
        .and_then(|a| a.data_retention_days)
        .unwrap_or(7);

    tracing::info!("调度系统就绪，开始监听时间段...");

    loop {
        if !once {
            let due = scheduler.get_due_periods().await;
            if due.is_empty() {
                tokio::time::sleep(Duration::from_secs(600)).await;
                continue;
            }

            tracing::info!("触发 {} 个调度周期", due.len());
            for period in &due {
                scheduler.mark_run(&period.id).await;
            }
        }

        tracing::info!("{}", "=".repeat(60));
        tracing::info!("开始新一轮数据采集 [模式: {}]", mode_label);
        tracing::info!("{}", "=".repeat(60));

        let start_time = Local::now();
        let max_retries = config
            .advanced
            .as_ref()
            .and_then(|a| a.max_retries)
            .unwrap_or(3);

        // 1. 爬取热榜数据
        let mut all_news: Vec<NewsItem> = Vec::new();
        let mut failed_ids: Vec<String> = Vec::new();

        if !platform_ids.is_empty() {
            tracing::info!("开始爬取 {} 个热榜平台...", platform_ids.len());
            match data_fetcher
                .fetch_all_platforms(&platform_ids, request_interval_ms, max_retries)
                .await
            {
                Ok(items) => {
                    tracing::info!("热榜爬取完成: {} 条", items.len());
                    all_news.extend(items);
                }
                Err(e) => {
                    tracing::error!("热榜爬取失败: {}", e);
                    for (id, _) in &platform_ids {
                        failed_ids.push(id.clone());
                    }
                }
            }
        }

        // 2. 爬取 RSS
        tracing::info!("开始爬取 RSS 源...");
        let rss_data = rss_fetcher.fetch_all().await;
        tracing::info!(
            "RSS 爬取完成: {} 条",
            rss_data.items.len()
        );

        // 3. 先快速入库检测是否有新内容（仅持续模式需要此优化）
        let mut total_new_items: usize = 0;
        if !once {
            let mut platform_groups_raw: std::collections::HashMap<String, Vec<&NewsItem>> =
                std::collections::HashMap::new();
            for item in &all_news {
                platform_groups_raw
                    .entry(item.platform.clone())
                    .or_default()
                    .push(item);
            }
            for (pid, items) in &platform_groups_raw {
                let items_vec: Vec<NewsItem> = items.iter().map(|&i| i.clone()).collect();
                if let Ok((new_count, _)) = storage.upsert_news_items(pid, pid, &items_vec) {
                    total_new_items += new_count;
                }
            }
            let mut feed_groups_raw: std::collections::HashMap<String, Vec<&RssItem>> =
                std::collections::HashMap::new();
            for item in &rss_data.items {
                let key = item.feed_name.clone();
                feed_groups_raw.entry(key).or_default().push(item);
            }
            for (feed_name, items) in &feed_groups_raw {
                let items_vec: Vec<RssItem> = items.iter().map(|&i| i.clone()).collect();
                if let Ok((new_count, _)) = storage.upsert_rss_items(feed_name, feed_name, &items_vec) {
                    total_new_items += new_count;
                }
            }
            if total_new_items == 0 {
                let elapsed = Local::now().signed_duration_since(start_time).num_seconds();
                tracing::info!("无新增内容，跳过 AI 筛选和推送");
                tracing::info!("本轮完成，耗时 {}s", elapsed);
                tokio::time::sleep(Duration::from_secs(600)).await;
                continue;
            }
            tracing::info!("检测到 {} 条新增内容，开始处理", total_new_items);
        }

        // 3. 关键词匹配
        let filter_method = config.filter.as_ref()
            .and_then(|f| f.method.clone())
            .unwrap_or_else(|| "keyword".to_string());
        let use_keyword_filter = filter_method != "ai";

        let (matched_news, mut matched_rss) = if use_keyword_filter {
            let mn = matcher.filter_news(&all_news);
            let mr = matcher.filter_rss(&rss_data.items);
            (mn, mr)
        } else {
            tracing::info!("筛选模式: AI 智能分类");
            let filter_cfg = config.ai_filter.as_ref();
            // 回退关键词匹配（和原版一样：AI 筛选失败时自动降级）
            let fallback = |all: &Vec<NewsItem>, rss: &Vec<RssItem>, reason: &str| {
                tracing::warn!("AI 筛选失败({}), 回退关键词匹配", reason);
                let default_kws: Vec<String> = Vec::new();
                let default_excludes: Vec<String> = Vec::new();
                if let Ok(m) = KeywordMatcher::new(&default_kws, &default_excludes) {
                    let mn = m.filter_news(all);
                    let mr = m.filter_rss(rss);
                    (mn, mr)
                } else {
                    (all.clone(), rss.clone())
                }
            };
            if let (Some(cfg), Some(_)) = (filter_cfg, &ai_client) {
                match AiFilter::new(cfg, config.ai.as_ref()) {
                    Ok(filter) => {
                        let interests_file = cfg.interests_file.as_deref().unwrap_or("ai_interests.txt");
                        let interests = std::fs::read_to_string(interests_file).unwrap_or_default();
                        match filter.extract_tags(&interests).await {
                            Ok(tags) => {
                                tracing::info!("AI 提取 {} 个标签", tags.len());
                                for tag in &tags { let _ = storage.upsert_ai_filter_tag(&tag.name); }
                                let min_score = cfg.min_score.unwrap_or(0.7);
                                let mut classified: Vec<NewsItem> = Vec::new();

                                // 跳过已分析过的新闻（省 token，原版特性）
                                let new_news: Vec<(usize, &NewsItem)> = all_news.iter().enumerate()
                                    .filter(|(_, n)| {
                                        !storage.is_news_analyzed_by_ai(n.url.as_deref().unwrap_or_default()).unwrap_or(false)
                                    })
                                    .collect();
                                let cached_count = all_news.len() - new_news.len();
                                if cached_count > 0 {
                                    tracing::info!("跳过 {} 条已分析新闻（缓存命中）", cached_count);
                                }

                                // 先恢复缓存中已有的标签
                                for (_, item) in all_news.iter().enumerate() {
                                    if storage.is_news_analyzed_by_ai(item.url.as_deref().unwrap_or_default()).unwrap_or(false) {
                                        let mut cached_item = item.clone();
                                        // 从数据库恢复标签
                                        if let Ok(tags) = storage.get_ai_filter_results(item.url.as_deref().unwrap_or("")) {
                                            if let Some((t, _, _)) = tags.first() {
                                                cached_item.keywords = vec![t.clone()];
                                            }
                                        }
                                        classified.push(cached_item);
                                    }
                                }

                                // 只对新新闻做 AI 分类
                                if !new_news.is_empty() {
                                    tracing::info!("AI 分类 {} 条新新闻 (共 {} 条)", new_news.len(), all_news.len());
                                    let new_titles: Vec<(i64, String)> = new_news.iter()
                                        .map(|(i, n)| ((*i + 1) as i64, n.title.clone())).collect();
                                    let class_results = filter.classify_batch(&tags, &new_titles).await;
                                    for class in &class_results {
                                        if class.score >= min_score {
                                                    if let Some(idx) = (class.news_id as usize).checked_sub(1) {
                                                        if let Some(item) = all_news.get(idx) {
                                                            let mut item = item.clone();
                                                            if let Some(tag) = tags.iter().find(|t| t.id == class.tag_id) {
                                                                item.keywords = vec![tag.name.clone()];
                                                                let _ = storage.insert_ai_filter_result(
                                                                    item.url.as_deref().unwrap_or(""), &tag.name, Some(class.score), None);
                                                                let _ = storage.mark_news_ai_analyzed(
                                                                        item.url.as_deref().unwrap_or(""));
                                                            }
                                                            if !classified.iter().any(|c: &NewsItem| c.url == item.url) {
                                                                classified.push(item);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            tracing::info!("AI 筛选新增 {} 条 (阈值: {})", class_results.len(), min_score);
                                }

                                tracing::info!("AI 筛选结果: {} 条 (含 {} 条缓存)", classified.len(), cached_count);
                                (classified, rss_data.items.clone())
                            }
                            Err(e) => fallback(&all_news, &rss_data.items, &format!("标签提取: {}", e))
                        }
                    }
                    Err(e) => fallback(&all_news, &rss_data.items, &format!("初始化: {}", e))
                }
            } else { fallback(&all_news, &rss_data.items, "未配置") }
        };

        tracing::info!(
            "过滤结果: 热榜 {}/{} 条, RSS {}/{} 条 [模式: {}]",
            matched_news.len(),
            all_news.len(),
            matched_rss.len(),
            rss_data.items.len(),
            filter_method
        );

        // 4. 存入数据库（按平台分组 upsert）
        let mut platform_groups: std::collections::HashMap<String, Vec<&NewsItem>> =
            std::collections::HashMap::new();
        for item in &matched_news {
            platform_groups
                .entry(item.platform.clone())
                .or_default()
                .push(item);
        }
        for (pid, items) in &platform_groups {
            let items_vec: Vec<NewsItem> = items.iter().map(|&i| i.clone()).collect();
            match storage.upsert_news_items(pid, pid, &items_vec) {
                Ok((new_count, updated_count)) => {
                    tracing::info!("平台 {} 入库: {} 新增 / {} 更新", pid, new_count, updated_count);
                }
                Err(e) => tracing::warn!("平台 {} 入库失败: {}", pid, e),
            }
        }

        // RSS 按 feed 分组
        let mut feed_groups: std::collections::HashMap<String, Vec<&RssItem>> =
            std::collections::HashMap::new();
        for item in &matched_rss {
            let key = item.feed_name.clone();
            feed_groups.entry(key).or_default().push(item);
        }
        for (feed_name, items) in &feed_groups {
            let items_vec: Vec<RssItem> = items.iter().map(|&i| i.clone()).collect();
            match storage.upsert_rss_items(feed_name, feed_name, &items_vec) {
                Ok((new_count, updated_count)) => {
                    tracing::info!("RSS {} 入库: {} 新增 / {} 更新", feed_name, new_count, updated_count);
                }
                Err(e) => tracing::warn!("RSS {} 入库失败: {}", feed_name, e),
            }
        }

        // 5. AI 分析 (如果已配置)
        let mut ai_analysis_result: Option<trendradar_core::ai::AiAnalysisResult> = None;
        if resolved.analyze && ai_client.is_some() {
            let analysis_config = config.ai_analysis.as_ref();
            if analysis_config.map(|a| a.enabled.unwrap_or(false)).unwrap_or(false) {
                tracing::info!("========== AI 深度分析 ==========");
                tracing::info!("待分析: {} 条热榜 + {} 条 RSS", matched_news.len(), matched_rss.len());

                let (system_prompt, user_prompt) = load_ai_prompt(
                    analysis_config.unwrap(),
                    &config.ai.as_ref(),
                    &base_dir,
                );

                let analyzer = AiAnalyzer::new(
                    analysis_config.unwrap(),
                    config.ai.as_ref(),
                    system_prompt,
                    user_prompt,
                )?;
                let query_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                tracing::info!("正在调用 AI API，请稍候...");
                match analyzer
                    .analyze(&matched_news, &matched_rss, &query_time)
                    .await
                {
                    Ok(result) => {
                        for item in &matched_news {
                            if let Some(ref url) = item.url {
                                let _ = storage.mark_news_ai_analyzed(url);
                            }
                        }
                        tracing::info!("AI 分析完成: 核心趋势 {} 字, 信号 {} 字",
                            result.core_trends.to_string().len(), result.signals.to_string().len());
                        ai_analysis_result = Some(result);
                    }
                    Err(e) => {
                        tracing::warn!("AI 分析失败: {}", e);
                    }
                }
                tracing::info!("========== AI 分析结束 ==========");
            } else {
                tracing::info!("AI 分析已禁用，跳过");
            }

            let filter_config = config.ai_filter.as_ref();
            if filter_config.map(|f| f.enabled.unwrap_or(false)).unwrap_or(false) {
                tracing::info!("AI 智能筛选功能已配置（待整合）");
            }

            let translate_config = config.ai_translation.as_ref();
            if translate_config.map(|t| t.enabled.unwrap_or(false)).unwrap_or(false) {
                tracing::info!("AI 翻译功能启动...");
                match AiTranslator::new(
                    translate_config.unwrap(),
                    config.ai.as_ref(),
                    translate_config.and_then(|t| t.prompt_file.as_deref()).unwrap_or("ai_translation_prompt.txt"),
                ) {
                    Ok(translator) => {
                        // 只翻译 RSS 标题（热榜标题已有中文，不需要翻译）
                        let rss_titles: Vec<String> = matched_rss.iter().map(|r| r.title.clone()).collect();
                        if !rss_titles.is_empty() {
                            tracing::info!("正在批量翻译 {} 条 RSS 标题...", rss_titles.len());
                            match translator.translate_batch(&rss_titles).await {
                                Ok(batch_result) => {
                                    for (i, result) in batch_result.results.iter().enumerate() {
                                        if i < matched_rss.len() && !result.translated.is_empty() && result.translated != result.original {
                                            matched_rss[i].title = format!("{} ({})", result.original, result.translated);
                                        }
                                    }
                                    tracing::info!("AI 翻译完成 {} 条", batch_result.results.len());
                                }
                                Err(e) => tracing::warn!("批量翻译失败: {}", e),
                            }
                        }
                    }
                    Err(e) => tracing::warn!("AI 翻译初始化失败: {}", e),
                }
            }
        } else {
            tracing::info!("AI 客户端未配置，跳过分析");
        }

        // 6. 构建报告数据
        tracing::info!("========== 构建报告 ==========");
        let mut stats_map: std::collections::BTreeMap<String, (usize, Vec<&NewsItem>)> =
            std::collections::BTreeMap::new();

        let has_keywords = matched_news.iter().any(|item| !item.keywords.is_empty());

        if has_keywords {
            for item in &matched_news {
                for kw in &item.keywords {
                    let entry = stats_map.entry(kw.clone()).or_insert((0, vec![]));
                    entry.0 += 1;
                    entry.1.push(item);
                }
            }
        } else {
            for item in &matched_news {
                let platform_label = item.platform_name.clone()
                    .unwrap_or_else(|| item.platform.clone());
                let entry = stats_map.entry(platform_label).or_insert((0, vec![]));
                entry.0 += 1;
                entry.1.push(item);
            }
        }

        let total_items = matched_news.len().max(1);
        let total_new = matched_news.iter().filter(|n| n.is_new.unwrap_or(false)).count();

        let mut stats: Vec<StatItem> = stats_map
            .into_iter()
            .map(|(word, (count, items))| {
                let titles: Vec<NewsDisplay> = items
                    .iter()
                    .take(display_settings.max_titles_per_keyword.unwrap_or(20))
                    .map(|item| NewsDisplay {
                        title: item.title.clone(),
                        source_name: item
                            .platform_name
                            .clone()
                            .unwrap_or_else(|| item.platform.clone()),
                        time_display: item
                            .publish_time
                            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_default(),
                        count,
                        ranks: item.rank.map(|r| vec![r as i32]).unwrap_or_default(),
                        rank_threshold: 5,
                        url: item.url.clone().unwrap_or_default(),
                        mobile_url: String::new(),
                        is_new: item.is_new.unwrap_or(false),
                    })
                    .collect();
                StatItem {
                    word,
                    count,
                    percentage: (count as f64 / total_items as f64) * 100.0,
                    titles,
                }
            })
            .collect();
        stats.sort_by(|a, b| b.count.cmp(&a.count));

        let generation_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let report_data = ReportData {
            title: format!("TrendRadar {}报告", mode_label),
            mode: mode_label.clone(),
            generation_time: generation_time.clone(),
            stats: stats.clone(),
            new_titles: vec![],
            failed_ids: failed_ids.clone(),
            total_new_count: total_new,
            total_items: matched_news.len(),
            update_info: None,
            ai_analysis: ai_analysis_result,
        };

        // 7. 生成报告文件
        let report_writer = ReportWriter::new(output_dir);
        let date_folder = Local::now().format("%Y-%m-%d").to_string();
        let time_filename = Local::now().format("%H-%M-%S").to_string();

        let md = generate_markdown_report(&report_data, &display_settings);
        let md_path = report_writer.write_snapshot(&date_folder, &time_filename, &md);
        tracing::info!("Markdown 报告: {}", md_path);

        let rss = generate_rss_report(&report_data, None);
        let rss_path = report_writer.write_latest("rss", &rss);
        tracing::info!("RSS 报告: {}", rss_path);

        let html = ReportTemplate::from_report_data(&report_data, &mode_label)
            .render()
            .unwrap_or_else(|e| {
                tracing::warn!("HTML 模板渲染失败: {}", e);
                format!(
                    "<!DOCTYPE html><html><head><meta charset=\"UTF-8\"><title>{}</title></head><body><pre>{}</pre></body></html>",
                    report_data.title, md
                )
            });
        let html_path = report_writer.write_latest(&mode_label, &html);
        tracing::info!("HTML 报告: {}", html_path);

        // 8. 发送通知
        if resolved.push {
            if let Some(ref notifier) = notifier {
            tracing::info!("========== 通知推送 ==========");
            let notification_content = generate_notification_content(
                &report_data,
                &display_settings,
                "",
            );

            if notification_content.trim().is_empty() && report_data.total_items == 0 {
                tracing::info!("无匹配内容，跳过通知推送");
            } else {
                tracing::info!("通知内容: {} 字", notification_content.len());
                let elapsed = Local::now()
                    .signed_duration_since(start_time)
                    .num_seconds();
                let footer = format!(
                    "\n\n---\n⏱ 耗时 {}s | 📊 共 {} 条 | 🆕 新增 {} 条",
                    elapsed, report_data.total_items, report_data.total_new_count
                );

                let full_content = format!("{}{}", notification_content, footer);

                tracing::info!("开始推送到通知渠道...");
                match notifier
                    .send_report(&full_content, &mode_label, &display_settings, Some(&html))
                    .await
                {
                    Ok(()) => tracing::info!("通知推送完成"),
                    Err(e) => tracing::warn!("通知推送出错: {}", e),
                }
            }
            } else {
                tracing::info!("通知模块未初始化，跳过推送");
            }
        } else {
            tracing::info!("[调度] 当前时间段不执行推送");
        }

        // 9. 清理过期数据
        match storage.delete_old_ai_filter_results(data_retention_days) {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("清理了 {} 条过期 AI 筛选结果", count);
                }
            }
            Err(e) => tracing::warn!("清理过期数据失败: {}", e),
        }
        let _ = storage.vacuum();

        let elapsed = Local::now()
            .signed_duration_since(start_time)
            .num_seconds();
        tracing::info!("本轮完成，耗时 {}s", elapsed);

        if once {
            tracing::info!("单次执行完成，退出");
            break;
        }

        tokio::time::sleep(Duration::from_secs(600)).await;
    }

    Ok(())
}
