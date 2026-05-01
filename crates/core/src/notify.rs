use crate::config::{
    AppConfig, BarkConfig, DingtalkConfig, EmailConfig, FeishuConfig, NtfyConfig, SlackConfig,
    TelegramConfig, WebhookConfig, WecomConfig,
};
use crate::error::{Result, TrendRadarError};
use crate::report::{split_content_by_bytes, get_batch_size_for_channel};
use regex::Regex;
use std::sync::Arc;
use std::sync::LazyLock;

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^\s<>\]]+").unwrap());
static BOLD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").unwrap());
static ITALIC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*(.+?)\*").unwrap());
static STRIKETHROUGH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~(.+?)~~").unwrap());
static IMG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[(.+?)\]\(.+?\)").unwrap());
static CODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`(.+?)`").unwrap());
static QUOTE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^>\s*").unwrap());
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#+\s*").unwrap());
static HR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[\-\*]{3,}\s*$").unwrap());
static FONT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<font[^>]*>(.+?)</font>").unwrap());
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static NEWLINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

static MRKDWN_LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
static MRKDWN_BOLD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap());

// ============================================================================
// Markdown 格式转换工具函数
// ============================================================================

pub fn strip_markdown(text: &str) -> String {
    let mut text = text.to_string();

    text = LINK_RE.replace_all(&text, "$1 $2").to_string();

    let mut protected_urls: Vec<String> = Vec::new();
    text = URL_RE
        .replace_all(&text, |caps: &regex::Captures| {
            let idx = protected_urls.len();
            protected_urls.push(caps[0].to_string());
            format!("@@URLTOKEN{idx}@@")
        })
        .to_string();

    text = BOLD_RE.replace_all(&text, "$1").to_string();
    text = ITALIC_RE.replace_all(&text, "$1").to_string();
    text = STRIKETHROUGH_RE.replace_all(&text, "$1").to_string();
    text = IMG_RE.replace_all(&text, "$1").to_string();
    text = CODE_RE.replace_all(&text, "$1").to_string();
    text = QUOTE_RE.replace_all(&text, "").to_string();
    text = HEADER_RE.replace_all(&text, "").to_string();
    text = HR_RE.replace_all(&text, "").to_string();
    text = FONT_RE.replace_all(&text, "$1").to_string();
    text = TAG_RE.replace_all(&text, "").to_string();
    text = NEWLINE_RE.replace_all(&text, "\n\n").to_string();

    for (idx, url) in protected_urls.iter().enumerate() {
        text = text.replace(&format!("@@URLTOKEN{idx}@@"), url);
    }

    text.trim().to_string()
}

pub fn convert_markdown_to_mrkdwn(content: &str) -> String {
    let mut content = content.to_string();

    content = MRKDWN_LINK_RE.replace_all(&content, "<$2|$1>").to_string();
    content = MRKDWN_BOLD_RE.replace_all(&content, "*$1*").to_string();

    content
}

// ============================================================================
// 批次头部工具函数
// ============================================================================

pub fn get_batch_header(format_type: &str, batch_num: usize, total_batches: usize) -> String {
    match format_type {
        "telegram" => format!("<b>[第 {}/{} 批次]</b>\n\n", batch_num, total_batches),
        "slack" => format!("*[第 {}/{} 批次]*\n\n", batch_num, total_batches),
        "wework_text" | "bark" => format!("[第 {}/{} 批次]\n\n", batch_num, total_batches),
        _ => format!("**[第 {}/{} 批次]**\n\n", batch_num, total_batches),
    }
}

pub fn add_batch_headers(
    batches: &[String],
    format_type: &str,
    _max_bytes: usize,
) -> Vec<String> {
    if batches.len() <= 1 {
        return batches.to_vec();
    }
    let total = batches.len();
    batches
        .iter()
        .enumerate()
        .map(|(i, content)| {
            let header = get_batch_header(format_type, i + 1, total);
            format!("{}{}", header, content)
        })
        .collect()
}

// ============================================================================
// SMTP 配置
// ============================================================================

pub struct SmtpInfo {
    pub server: &'static str,
    pub port: u16,
    pub encryption: SmtpEncryption,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmtpEncryption {
    Tls,
    Ssl,
}

pub fn get_smtp_config(domain: &str) -> SmtpInfo {
    match domain {
        "gmail.com" => SmtpInfo {
            server: "smtp.gmail.com",
            port: 587,
            encryption: SmtpEncryption::Tls,
        },
        "qq.com" => SmtpInfo {
            server: "smtp.qq.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "outlook.com" | "hotmail.com" | "live.com" => SmtpInfo {
            server: "smtp-mail.outlook.com",
            port: 587,
            encryption: SmtpEncryption::Tls,
        },
        "163.com" => SmtpInfo {
            server: "smtp.163.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "126.com" => SmtpInfo {
            server: "smtp.126.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "sina.com" => SmtpInfo {
            server: "smtp.sina.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "sohu.com" => SmtpInfo {
            server: "smtp.sohu.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "189.cn" => SmtpInfo {
            server: "smtp.189.cn",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "aliyun.com" => SmtpInfo {
            server: "smtp.aliyun.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "yandex.com" => SmtpInfo {
            server: "smtp.yandex.com",
            port: 465,
            encryption: SmtpEncryption::Ssl,
        },
        "icloud.com" => SmtpInfo {
            server: "smtp.mail.me.com",
            port: 587,
            encryption: SmtpEncryption::Tls,
        },
        _ => SmtpInfo {
            server: "",
            port: 587,
            encryption: SmtpEncryption::Tls,
        },
    }
}

// ============================================================================
// 内部 HTTP 请求辅助函数
// ============================================================================

async fn http_post_json(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
) -> Result<reqwest::Response> {
    let resp = client
        .post(url)
        .json(payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| TrendRadarError::Notification(format!("HTTP request failed: {}", e)))?;
    Ok(resp)
}

async fn http_post_body(
    client: &reqwest::Client,
    url: &str,
    body: String,
    headers: Vec<(&str, String)>,
) -> Result<reqwest::Response> {
    let mut req = client
        .post(url)
        .body(body)
        .timeout(std::time::Duration::from_secs(30));

    for (k, v) in headers {
        req = req.header(k, v);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| TrendRadarError::Notification(format!("HTTP request failed: {}", e)))?;
    Ok(resp)
}

fn build_proxy_client(proxy_url: Option<&str>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(url) = proxy_url {
        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| TrendRadarError::Notification(format!("Invalid proxy URL: {}", e)))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|e| TrendRadarError::Notification(format!("Failed to build HTTP client: {}", e)))
}

// ============================================================================
// 飞书 (Feishu)
// ============================================================================

pub async fn send_to_feishu(
    config: &FeishuConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "飞书".to_string()
    } else {
        format!("飞书{}", account_label)
    };

    let content_size = content.len();
    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    let payload = if config.webhook_url.contains("www.feishu.cn") {
        serde_json::json!({
            "msg_type": "text",
            "content": {
                "text": content
            }
        })
    } else {
        serde_json::json!({
            "msg_type": "interactive",
            "card": {
                "schema": "2.0",
                "body": {
                    "elements": [
                        {"tag": "markdown", "content": content}
                    ]
                }
            }
        })
    };

    let resp = http_post_json(&client, &config.webhook_url, &payload).await?;
    if resp.status().as_u16() == 200 {
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TrendRadarError::Notification(format!("Failed to parse response: {}", e)))?;
        let status_ok = result.get("StatusCode").and_then(|v| v.as_i64()) == Some(0)
            || result.get("code").and_then(|v| v.as_i64()) == Some(0);
        if status_ok {
            println!(
                "{}第 {}/{} 批次发送成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            return Ok(());
        }
        let error_msg = result
            .get("msg")
            .or_else(|| result.get("StatusMessage"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        Err(TrendRadarError::Notification(format!(
            "{}发送失败: {}",
            log_prefix, error_msg
        )))
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// 钉钉 (DingTalk)
// ============================================================================

pub async fn send_to_dingtalk(
    config: &DingtalkConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "钉钉".to_string()
    } else {
        format!("钉钉{}", account_label)
    };

    let content_size = content.len();
    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    let payload = serde_json::json!({
        "msgtype": "markdown",
        "markdown": {
            "title": format!("TrendRadar 热点分析报告 - {}", report_type),
            "text": content
        }
    });

    let resp = http_post_json(&client, &config.webhook_url, &payload).await?;
    if resp.status().as_u16() == 200 {
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TrendRadarError::Notification(format!("Failed to parse response: {}", e)))?;
        if result.get("errcode").and_then(|v| v.as_i64()) == Some(0) {
            println!(
                "{}第 {}/{} 批次发送成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            return Ok(());
        }
        let error_msg = result
            .get("errmsg")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        Err(TrendRadarError::Notification(format!(
            "{}发送失败: {}",
            log_prefix, error_msg
        )))
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// 企业微信 (WeCom)
// ============================================================================

pub async fn send_to_wecom(
    config: &WecomConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
    msg_type: &str,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "企业微信".to_string()
    } else {
        format!("企业微信{}", account_label)
    };

    let is_text_mode = msg_type.to_lowercase() == "text";

    let (payload, content_size) = if is_text_mode {
        let plain = strip_markdown(content);
        let size = plain.len();
        (
            serde_json::json!({
                "msgtype": "text",
                "text": {"content": plain}
            }),
            size,
        )
    } else {
        (
            serde_json::json!({
                "msgtype": "markdown",
                "markdown": {"content": content}
            }),
            content.len(),
        )
    };

    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    let resp = http_post_json(&client, &config.webhook_url, &payload).await?;
    if resp.status().as_u16() == 200 {
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TrendRadarError::Notification(format!("Failed to parse response: {}", e)))?;
        if result.get("errcode").and_then(|v| v.as_i64()) == Some(0) {
            println!(
                "{}第 {}/{} 批次发送成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            return Ok(());
        }
        let error_msg = result
            .get("errmsg")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        Err(TrendRadarError::Notification(format!(
            "{}发送失败: {}",
            log_prefix, error_msg
        )))
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// Telegram
// ============================================================================

pub async fn send_to_telegram(
    config: &TelegramConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "Telegram".to_string()
    } else {
        format!("Telegram{}", account_label)
    };

    let url = format!(
        "https://api.telegram.org/bot{}/sendMessage",
        config.bot_token
    );

    let content_size = content.len();
    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    let payload = serde_json::json!({
        "chat_id": config.chat_id,
        "text": content,
        "parse_mode": "HTML",
        "disable_web_page_preview": true
    });

    let resp = http_post_json(&client, &url, &payload).await?;
    if resp.status().as_u16() == 200 {
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TrendRadarError::Notification(format!("Failed to parse response: {}", e)))?;
        if result.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            println!(
                "{}第 {}/{} 批次发送成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            return Ok(());
        }
        let error_msg = result
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        Err(TrendRadarError::Notification(format!(
            "{}发送失败: {}",
            log_prefix, error_msg
        )))
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// 邮件 (Email)
// ============================================================================

pub async fn send_to_email(
    config: &EmailConfig,
    report_type: &str,
    html_content: &str,
    now: &str,
) -> Result<()> {
    use lettre::message::{header, Mailbox, MessageBuilder, MultiPart, SinglePart};
    use lettre::AsyncSmtpTransport;
    use lettre::AsyncTransport;

    let from_addr = &config.from;
    let domain = from_addr
        .split('@')
        .nth(1)
        .unwrap_or("unknown")
        .to_lowercase();

    let smtp = if let Some(port) = config.smtp_port {
        SmtpInfo {
            server: "",
            port,
            encryption: if port == 465 {
                SmtpEncryption::Ssl
            } else {
                SmtpEncryption::Tls
            },
        }
    } else {
        get_smtp_config(&domain)
    };

    let smtp_server = if smtp.server.is_empty() {
        config.smtp_server.as_deref().unwrap_or("").to_string()
    } else {
        smtp.server.to_string()
    };

    let smtp_port = config.smtp_port.unwrap_or(smtp.port);

    let to_addrs: Vec<String> = config.to.split(',').map(|s| s.trim().to_string()).collect();

    println!("正在发送邮件到 {:?}...", to_addrs);
    println!("SMTP 服务器: {}:{}", smtp_server, smtp_port);
    println!("发件人: {}", from_addr);

    let from: Mailbox = format!("TrendRadar <{}>", from_addr)
        .parse()
        .map_err(|e| TrendRadarError::Notification(format!("Invalid from address: {}", e)))?;

    let subject = format!(
        "TrendRadar 热点分析报告 - {} - {}",
        report_type, now
    );

    let text_content = format!(
        "TrendRadar 热点分析报告\n========================\n报告类型：{}\n生成时间：{}\n\n请使用支持HTML的邮件客户端查看完整报告内容。",
        report_type, now
    );

    let mut msg_builder = MessageBuilder::new();
    msg_builder = msg_builder.from(from);

    for to_addr in &to_addrs {
        let to: Mailbox = to_addr
            .parse()
            .map_err(|e| TrendRadarError::Notification(format!("Invalid to address: {}", e)))?;
        msg_builder = msg_builder.to(to);
    }

    msg_builder = msg_builder.subject(subject);
    msg_builder = msg_builder.header(header::MIME_VERSION_1_0);

    let msg = msg_builder
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_PLAIN)
                        .header(header::ContentTransferEncoding::QuotedPrintable)
                        .body(text_content),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_HTML)
                        .header(header::ContentTransferEncoding::QuotedPrintable)
                        .body(html_content.to_string()),
                ),
        )
        .map_err(|e| TrendRadarError::Notification(format!("Failed to build email: {}", e)))?;

    let creds = lettre::transport::smtp::authentication::Credentials::new(
        config.from.clone(),
        config.password.clone(),
    );

    let transport_builder = if smtp.encryption == SmtpEncryption::Ssl {
        AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(&smtp_server)
    } else {
        AsyncSmtpTransport::<lettre::Tokio1Executor>::starttls_relay(&smtp_server)
    };

    let mailer = transport_builder
        .map_err(|e| TrendRadarError::Notification(format!("SMTP setup failed: {}", e)))?
        .port(smtp_port)
        .credentials(creds)
        .build();

    mailer
        .send(msg)
        .await
        .map_err(|e| TrendRadarError::Notification(format!("邮件发送失败: {}", e)))?;

    println!(
        "邮件发送成功 [{}] -> {:?}",
        report_type, to_addrs
    );
    Ok(())
}

// ============================================================================
// ntfy
// ============================================================================

pub async fn send_to_ntfy(
    config: &NtfyConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "ntfy".to_string()
    } else {
        format!("ntfy{}", account_label)
    };

    let report_type_en = match report_type {
        "全天汇总" => "Daily Summary",
        "当前榜单" => "Current Ranking",
        "增量分析" => "Incremental Update",
        "通知连通性测试" => "Notification Test",
        _ => "News Report",
    };

    let base_url = config.server_url.trim_end_matches('/');
    let base_url = if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        format!("https://{}", base_url)
    } else {
        base_url.to_string()
    };
    let url = format!("{}/{}", base_url, config.topic);

    let content_size = content.len();
    let mut headers: Vec<(&str, String)> = vec![
        ("Content-Type", "text/plain; charset=utf-8".to_string()),
        ("Markdown", "yes".to_string()),
        ("Priority", "default".to_string()),
        ("Tags", "news".to_string()),
    ];

    if total_batches > 1 {
        headers.push((
            "Title",
            format!("{} ({}/{})", report_type_en, batch_num, total_batches),
        ));
    } else {
        headers.push(("Title", report_type_en.to_string()));
    }

    if let Some(ref token) = config.token {
        headers.push(("Authorization", format!("Bearer {}", token)));
    }

    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    if content_size > 4096 {
        println!(
            "警告：{}第 {} 批次消息过大（{} 字节），可能被拒绝",
            log_prefix, batch_num, content_size
        );
    }

    let resp = http_post_body(&client, &url, content.to_string(), headers).await?;

    if resp.status().as_u16() == 200 {
        println!(
            "{}第 {}/{} 批次发送成功 [{}]",
            log_prefix, batch_num, total_batches, report_type
        );
        Ok(())
    } else if resp.status().as_u16() == 429 {
        println!(
            "{}第 {}/{} 批次速率限制 [{}]，等待后重试",
            log_prefix, batch_num, total_batches, report_type
        );
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let mut retry_headers: Vec<(&str, String)> = if total_batches > 1 {
            vec![
                ("Content-Type", "text/plain; charset=utf-8".to_string()),
                ("Markdown", "yes".to_string()),
                ("Title", format!("{} ({}/{})", report_type_en, batch_num, total_batches)),
                ("Priority", "default".to_string()),
                ("Tags", "news".to_string()),
            ]
        } else {
            vec![
                ("Content-Type", "text/plain; charset=utf-8".to_string()),
                ("Markdown", "yes".to_string()),
                ("Title", report_type_en.to_string()),
                ("Priority", "default".to_string()),
                ("Tags", "news".to_string()),
            ]
        };
        if let Some(ref token) = config.token {
            retry_headers.push(("Authorization", format!("Bearer {}", token)));
        }
        let retry_resp = http_post_body(&client, &url, content.to_string(), retry_headers).await?;
        if retry_resp.status().as_u16() == 200 {
            println!(
                "{}第 {}/{} 批次重试成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            Ok(())
        } else {
            Err(TrendRadarError::Notification(format!(
                "{}重试失败，状态码: {}",
                log_prefix,
                retry_resp.status()
            )))
        }
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// Bark
// ============================================================================

pub async fn send_to_bark(
    config: &BarkConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "Bark".to_string()
    } else {
        format!("Bark{}", account_label)
    };

    let content_size = content.len();
    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    if content_size > 4096 {
        println!(
            "警告：{}第 {} 批次消息过大（{} 字节），可能被拒绝",
            log_prefix, batch_num, content_size
        );
    }

    let payload = serde_json::json!({
        "title": report_type,
        "markdown": content,
        "sound": "default",
        "group": "TrendRadar",
        "action": "none"
    });

    let resp = http_post_json(&client, &config.url, &payload).await?;
    if resp.status().as_u16() == 200 {
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TrendRadarError::Notification(format!("Failed to parse response: {}", e)))?;
        if result.get("code").and_then(|v| v.as_i64()) == Some(200) {
            println!(
                "{}第 {}/{} 批次发送成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            return Ok(());
        }
        let error_msg = result
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        Err(TrendRadarError::Notification(format!(
            "{}发送失败: {}",
            log_prefix, error_msg
        )))
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// Slack
// ============================================================================

pub async fn send_to_slack(
    config: &SlackConfig,
    content: &str,
    report_type: &str,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "Slack".to_string()
    } else {
        format!("Slack{}", account_label)
    };

    let mrkdwn_content = convert_markdown_to_mrkdwn(content);
    let content_size = mrkdwn_content.len();

    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    let payload = serde_json::json!({
        "text": mrkdwn_content
    });

    let resp = http_post_json(&client, &config.webhook_url, &payload).await?;
    if resp.status().as_u16() == 200 {
        let body = resp.text().await.unwrap_or_default();
        if body == "ok" {
            println!(
                "{}第 {}/{} 批次发送成功 [{}]",
                log_prefix, batch_num, total_batches, report_type
            );
            return Ok(());
        }
        Err(TrendRadarError::Notification(format!(
            "{}发送失败: {}",
            log_prefix, body
        )))
    } else {
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}",
            log_prefix,
            resp.status()
        )))
    }
}

// ============================================================================
// 通用 Webhook
// ============================================================================

pub async fn send_to_webhook(
    config: &WebhookConfig,
    content: &str,
    report_type: &str,
    payload_template: Option<&str>,
    proxy_url: Option<&str>,
    account_label: &str,
    batch_num: usize,
    total_batches: usize,
) -> Result<()> {
    let client = build_proxy_client(proxy_url)?;
    let log_prefix = if account_label.is_empty() {
        "通用Webhook".to_string()
    } else {
        format!("通用Webhook{}", account_label)
    };

    let content_size = content.len();
    println!(
        "发送{}第 {}/{} 批次，大小：{} 字节 [{}]",
        log_prefix, batch_num, total_batches, content_size, report_type
    );

    let payload = if let Some(template) = payload_template {
        let escaped_content = serde_json::to_string(content)
            .unwrap_or_default();
        let escaped_content = escaped_content
            .trim_start_matches('"')
            .trim_end_matches('"');
        let escaped_title = serde_json::to_string(report_type)
            .unwrap_or_default();
        let escaped_title = escaped_title
            .trim_start_matches('"')
            .trim_end_matches('"');

        let payload_str = template
            .replace("{content}", escaped_content)
            .replace("{title}", escaped_title);

        match serde_json::from_str::<serde_json::Value>(&payload_str) {
            Ok(v) => v,
            Err(e) => {
                println!("{} JSON 模板解析失败: {}，回退到默认格式", log_prefix, e);
                serde_json::json!({
                    "title": report_type,
                    "content": content
                })
            }
        }
    } else {
        serde_json::json!({
            "title": report_type,
            "content": content
        })
    };

    let resp = http_post_json(&client, &config.webhook_url, &payload).await?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        println!(
            "{}第 {}/{} 批次发送成功 [{}]",
            log_prefix, batch_num, total_batches, report_type
        );
        Ok(())
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(TrendRadarError::Notification(format!(
            "{}发送失败，状态码: {}, 响应: {}",
            log_prefix, status, body
        )))
    }
}

// ============================================================================
// 统一调度：批量发送
// ============================================================================

pub struct BatchNotifyParams<'a> {
    pub batches: &'a [String],
    pub report_type: &'a str,
    pub proxy_url: Option<&'a str>,
    pub account_label: &'a str,
    pub batch_interval_secs: f64,
}

pub async fn send_feishu_batch(
    config: &FeishuConfig,
    params: BatchNotifyParams<'_>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "feishu", 29000);
    let total = batches.len();
    println!(
        "飞书{}消息分为 {} 批次发送 [{}]",
        params.account_label, total, params.report_type
    );
    for (i, content) in batches.iter().enumerate() {
        send_to_feishu(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            i + 1,
            total,
        )
        .await?;
        if i + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "飞书{}所有 {} 批次发送完成 [{}]",
        params.account_label, total, params.report_type
    );
    Ok(())
}

pub async fn send_dingtalk_batch(
    config: &DingtalkConfig,
    params: BatchNotifyParams<'_>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "dingtalk", 20000);
    let total = batches.len();
    println!(
        "钉钉{}消息分为 {} 批次发送 [{}]",
        params.account_label, total, params.report_type
    );
    for (i, content) in batches.iter().enumerate() {
        send_to_dingtalk(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            i + 1,
            total,
        )
        .await?;
        if i + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "钉钉{}所有 {} 批次发送完成 [{}]",
        params.account_label, total, params.report_type
    );
    Ok(())
}

pub async fn send_wecom_batch(
    config: &WecomConfig,
    params: BatchNotifyParams<'_>,
    msg_type: &str,
) -> Result<()> {
    let format_type = if msg_type == "text" {
        "wework_text"
    } else {
        "wework"
    };
    let batches = add_batch_headers(params.batches, format_type, 4000);
    let total = batches.len();
    let log_prefix = if params.account_label.is_empty() {
        "企业微信".to_string()
    } else {
        format!("企业微信{}", params.account_label)
    };
    println!(
        "{}消息分为 {} 批次发送 [{}]",
        log_prefix, total, params.report_type
    );
    for (i, content) in batches.iter().enumerate() {
        send_to_wecom(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            i + 1,
            total,
            msg_type,
        )
        .await?;
        if i + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "{}所有 {} 批次发送完成 [{}]",
        log_prefix, total, params.report_type
    );
    Ok(())
}

pub async fn send_telegram_batch(
    config: &TelegramConfig,
    params: BatchNotifyParams<'_>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "telegram", 4000);
    let total = batches.len();
    let log_prefix = if params.account_label.is_empty() {
        "Telegram".to_string()
    } else {
        format!("Telegram{}", params.account_label)
    };
    println!(
        "{}消息分为 {} 批次发送 [{}]",
        log_prefix, total, params.report_type
    );
    for (i, content) in batches.iter().enumerate() {
        send_to_telegram(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            i + 1,
            total,
        )
        .await?;
        if i + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "{}所有 {} 批次发送完成 [{}]",
        log_prefix, total, params.report_type
    );
    Ok(())
}

pub async fn send_ntfy_batch(
    config: &NtfyConfig,
    params: BatchNotifyParams<'_>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "ntfy", 3800);
    let total = batches.len();
    let log_prefix = if params.account_label.is_empty() {
        "ntfy".to_string()
    } else {
        format!("ntfy{}", params.account_label)
    };
    println!(
        "{}消息分为 {} 批次发送 [{}]",
        log_prefix, total, params.report_type
    );
    let reversed: Vec<&String> = batches.iter().rev().collect();
    for (idx, content) in reversed.iter().enumerate() {
        let actual_batch_num = total - idx;
        send_to_ntfy(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            actual_batch_num,
            total,
        )
        .await?;
        if idx + 1 < total {
            let interval = if config.server_url.contains("ntfy.sh") {
                2.0
            } else {
                1.0
            };
            tokio::time::sleep(std::time::Duration::from_secs_f64(interval)).await;
        }
    }
    println!(
        "{}所有 {} 批次发送完成 [{}]",
        log_prefix, total, params.report_type
    );
    Ok(())
}

pub async fn send_bark_batch(
    config: &BarkConfig,
    params: BatchNotifyParams<'_>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "bark", 3600);
    let total = batches.len();
    let log_prefix = if params.account_label.is_empty() {
        "Bark".to_string()
    } else {
        format!("Bark{}", params.account_label)
    };
    println!(
        "{}消息分为 {} 批次发送 [{}]",
        log_prefix, total, params.report_type
    );
    let reversed: Vec<&String> = batches.iter().rev().collect();
    for (idx, content) in reversed.iter().enumerate() {
        let actual_batch_num = total - idx;
        send_to_bark(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            actual_batch_num,
            total,
        )
        .await?;
        if idx + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "{}所有 {} 批次发送完成 [{}]",
        log_prefix, total, params.report_type
    );
    Ok(())
}

pub async fn send_slack_batch(
    config: &SlackConfig,
    params: BatchNotifyParams<'_>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "slack", 4000);
    let total = batches.len();
    let log_prefix = if params.account_label.is_empty() {
        "Slack".to_string()
    } else {
        format!("Slack{}", params.account_label)
    };
    println!(
        "{}消息分为 {} 批次发送 [{}]",
        log_prefix, total, params.report_type
    );
    for (i, content) in batches.iter().enumerate() {
        send_to_slack(
            config,
            content,
            params.report_type,
            params.proxy_url,
            params.account_label,
            i + 1,
            total,
        )
        .await?;
        if i + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "{}所有 {} 批次发送完成 [{}]",
        log_prefix, total, params.report_type
    );
    Ok(())
}

pub async fn send_webhook_batch(
    config: &WebhookConfig,
    params: BatchNotifyParams<'_>,
    payload_template: Option<&str>,
) -> Result<()> {
    let batches = add_batch_headers(params.batches, "wework", 4000);
    let total = batches.len();
    let log_prefix = if params.account_label.is_empty() {
        "通用Webhook".to_string()
    } else {
        format!("通用Webhook{}", params.account_label)
    };
    println!(
        "{}消息分为 {} 批次发送 [{}]",
        log_prefix, total, params.report_type
    );
    for (i, content) in batches.iter().enumerate() {
        send_to_webhook(
            config,
            content,
            params.report_type,
            payload_template,
            params.proxy_url,
            params.account_label,
            i + 1,
            total,
        )
        .await?;
        if i + 1 < total {
            tokio::time::sleep(std::time::Duration::from_secs_f64(params.batch_interval_secs))
                .await;
        }
    }
    println!(
        "{}所有 {} 批次发送完成 [{}]",
        log_prefix, total, params.report_type
    );
    Ok(())
}

// ============================================================================
// 统一通知器
// ============================================================================

pub struct Notifier {
    config: Arc<AppConfig>,
}

impl Notifier {
    pub fn new(config: Arc<AppConfig>) -> Result<Self> {
        Ok(Notifier { config })
    }

    #[allow(clippy::while_let_on_iterator)]
    pub async fn send_report(
        &self,
        content: &str,
        report_type: &str,
        _display_settings: &crate::config::DisplaySettings,
        full_html: Option<&str>,
    ) -> Result<()> {
        let notification = self.config.notification.as_ref();
        if notification.is_none() {
            tracing::info!("通知模块未配置，跳过推送");
            return Ok(());
        }
        let notification = notification.unwrap();

        if !notification.enabled.unwrap_or(false) {
            tracing::info!("通知模块已禁用");
            return Ok(());
        }

        let mut channels: Vec<String> = Vec::new();
        for (name, configured) in notification.channels.effective_channels() {
            if configured {
                channels.push(name.to_string());
            }
        }

        if channels.is_empty() {
            return Ok(());
        }

        let proxy_url: Option<&str> = self.config.advanced.as_ref()
            .and_then(|a| a.proxy_url.as_deref());

        for channel in &channels {
            let batch_size = get_batch_size_for_channel(channel);
            let batches = split_content_by_bytes(content, batch_size);

            if batches.is_empty() {
                continue;
            }

            let total = batches.len();
            let channel_label = channel.clone();
            let channel_lower = channel_label.to_lowercase();

            tracing::info!("[{}] 准备发送 {} 批次", channel_label, total);

            let batch_result = match channel_lower.as_str() {
                "feishu" if !notification.channels.feishu.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_feishu(cfg, content, rt, px, label, bn, tb))
                        },
                        &notification.channels.feishu,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "dingtalk" if !notification.channels.dingtalk.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_dingtalk(cfg, content, rt, px, label, bn, tb))
                        },
                        &notification.channels.dingtalk,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "wecom" if !notification.channels.wework.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_wecom(cfg, content, rt, px, label, bn, tb, "markdown"))
                        },
                        &notification.channels.wework,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "telegram" if !notification.channels.telegram.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_telegram(cfg, content, rt, px, label, bn, tb))
                        },
                        &notification.channels.telegram,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "email" if !notification.channels.email.is_empty() => {
                    let email_html = if let Some(html) = full_html {
                        html.to_string()
                    } else {
                        crate::report::markdown_to_simple_html(&batches.concat())
                    };
                    let email_batches = vec![email_html];
                    let (success, fail) = self.send_email_batches(
                        &email_batches, report_type, &notification.channels.email,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "bark" if !notification.channels.bark.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_bark(cfg, content, rt, px, label, bn, tb))
                        },
                        &notification.channels.bark,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "ntfy" if !notification.channels.ntfy.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_ntfy(cfg, content, rt, px, label, bn, tb))
                        },
                        &notification.channels.ntfy,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "slack" if !notification.channels.slack.is_empty() => {
                    let (success, fail) = self.send_channel_batches(
                        &batches, report_type, proxy_url,
                        |cfg, content, rt, px, label, bn, tb| {
                            Box::pin(crate::notify::send_to_slack(cfg, content, rt, px, label, bn, tb))
                        },
                        &notification.channels.slack,
                    ).await;
                    Ok::<_, TrendRadarError>((success, fail))
                }
                "webhook" if !notification.channels.generic_webhook.is_empty() => {
                    let (success, fail) = self.send_webhook_batches(
                        &batches, report_type, &notification.channels.generic_webhook,
                    ).await;
                    Ok((success, fail))
                }
                ch if !ch.is_empty() => {
                    tracing::info!("[{}] 未知或不支持批量发送的渠道", ch);
                    Ok((0, 0))
                }
                _ => {
                    tracing::info!("[{}] 该渠道未配置", channel_label);
                    Ok((0, 0))
                }
            };

            match batch_result {
                Ok((_success, _fail)) => {
                    tracing::info!("[{}] 通知发送完成", channel_label);
                }
                Err(e) => {
                    tracing::warn!("[{}] 通知发送出错: {}", channel_label, e);
                }
            }
        }

        Ok(())
    }

    async fn send_channel_batches<T, F>(
        &self,
        batches: &[String],
        report_type: &str,
        proxy_url: Option<&str>,
        send_fn: F,
        configs: &[T],
    ) -> (usize, usize)
    where
        F: for<'a> Fn(&'a T, &'a str, &'a str, Option<&'a str>, &'a str, usize, usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>>,
    {
        let total_batches = batches.len();
        let mut success = 0;
        let mut failed = 0;

        for (account_idx, config) in configs.iter().enumerate() {
            let label = if configs.len() > 1 {
                format!("-account{}", account_idx + 1)
            } else {
                String::new()
            };

            for (i, content) in batches.iter().enumerate() {
                match send_fn(config, content, report_type, proxy_url, &label, i + 1, total_batches).await {
                    Ok(()) => success += 1,
                    Err(e) => {
                        failed += 1;
                        tracing::warn!("发送失败: {}", e);
                    }
                }

                if i + 1 < total_batches {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }

            if account_idx + 1 < configs.len() {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }

        (success, failed)
    }

    async fn send_email_batches(
        &self,
        html_batches: &[String],
        report_type: &str,
        configs: &[EmailConfig],
    ) -> (usize, usize) {
        let mut success = 0;
        let mut failed = 0;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        for (account_idx, config) in configs.iter().enumerate() {
            let label = if configs.len() > 1 {
                format!("-account{}", account_idx + 1)
            } else {
                String::new()
            };

            let log_prefix = if label.is_empty() {
                "邮件".to_string()
            } else {
                format!("邮件{}", label)
            };

            for (i, html_content) in html_batches.iter().enumerate() {
                let batch_num = i + 1;
                let total_batches = html_batches.len();
                tracing::info!(
                    "发送{}第 {}/{} 批次 [{}]",
                    log_prefix, batch_num, total_batches, report_type
                );

                match send_to_email(config, report_type, html_content, &now).await {
                    Ok(()) => {
                        tracing::info!(
                            "{}第 {}/{} 批次发送成功 [{}]",
                            log_prefix, batch_num, total_batches, report_type
                        );
                        success += 1;
                    }
                    Err(e) => {
                        failed += 1;
                        tracing::warn!("{}发送失败: {}", log_prefix, e);
                    }
                }

                if i + 1 < html_batches.len() {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }

        (success, failed)
    }

    async fn send_webhook_batches(
        &self,
        batches: &[String],
        report_type: &str,
        configs: &[WebhookConfig],
    ) -> (usize, usize) {
        let mut success = 0;
        let mut failed = 0;

        for (account_idx, config) in configs.iter().enumerate() {
            let label = if configs.len() > 1 {
                format!("-account{}", account_idx + 1)
            } else {
                String::new()
            };
            let log_prefix = if label.is_empty() {
                "通用Webhook".to_string()
            } else {
                format!("通用Webhook{}", label)
            };

            for (i, content) in batches.iter().enumerate() {
                let batch_num = i + 1;
                let total_batches = batches.len();
                match send_to_webhook(
                    config,
                    content,
                    report_type,
                    config.payload_template.as_deref(),
                    None,
                    &label,
                    batch_num,
                    total_batches,
                )
                .await
                {
                    Ok(()) => {
                        tracing::info!(
                            "{}第 {}/{} 批次发送成功 [{}]",
                            log_prefix, batch_num, total_batches, report_type
                        );
                        success += 1;
                    }
                    Err(e) => {
                        failed += 1;
                        tracing::warn!("{}发送失败: {}", log_prefix, e);
                    }
                }

                if i + 1 < batches.len() {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }

        (success, failed)
    }

    pub async fn test_channel(&self, channel: &str) -> Result<()> {
        let notification = self.config.notification.as_ref()
            .ok_or_else(|| TrendRadarError::Config("通知模块未配置".to_string()))?;

        let test_content = "🔔 TrendRadar 测试消息\n\n如果您收到此消息，说明通知渠道配置正确！";

        let ch = &notification.channels;
        match channel.to_lowercase().as_str() {
            "feishu" => {
                let cfg = ch.feishu.first()
                    .ok_or_else(|| TrendRadarError::Config("飞书未配置".to_string()))?;
                send_to_feishu(cfg, test_content, "test", None, "", 1, 1).await
            }
            "dingtalk" => {
                let cfg = ch.dingtalk.first()
                    .ok_or_else(|| TrendRadarError::Config("钉钉未配置".to_string()))?;
                send_to_dingtalk(cfg, test_content, "test", None, "", 1, 1).await
            }
            "wecom" => {
                let cfg = ch.wework.first()
                    .ok_or_else(|| TrendRadarError::Config("企业微信未配置".to_string()))?;
                send_to_wecom(cfg, test_content, "test", None, "", 1, 1, "markdown").await
            }
            "telegram" => {
                let cfg = ch.telegram.first()
                    .ok_or_else(|| TrendRadarError::Config("Telegram未配置".to_string()))?;
                send_to_telegram(cfg, test_content, "test", None, "", 1, 1).await
            }
            "bark" => {
                let cfg = ch.bark.first()
                    .ok_or_else(|| TrendRadarError::Config("Bark未配置".to_string()))?;
                send_to_bark(cfg, test_content, "test", None, "", 1, 1).await
            }
            "ntfy" => {
                let cfg = ch.ntfy.first()
                    .ok_or_else(|| TrendRadarError::Config("ntfy未配置".to_string()))?;
                send_to_ntfy(cfg, test_content, "test", None, "", 1, 1).await
            }
            "slack" => {
                let cfg = ch.slack.first()
                    .ok_or_else(|| TrendRadarError::Config("Slack未配置".to_string()))?;
                send_to_slack(cfg, test_content, "test", None, "", 1, 1).await
            }
            "webhook" => {
                let cfg = ch.generic_webhook.first()
                    .ok_or_else(|| TrendRadarError::Config("Webhook未配置".to_string()))?;
                send_to_webhook(cfg, test_content, "test", None, None, "", 1, 1).await
            }
            "email" => {
                let cfg = ch.email.first()
                    .ok_or_else(|| TrendRadarError::Config("邮件未配置".to_string()))?;
                let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let html = format!("<html><body><h1>TrendRadar 测试</h1><p>如果您收到此邮件，说明通知渠道配置正确！</p><p>时间: {}</p></body></html>", now);
                send_to_email(cfg, "test", &html, &now).await
            }
            _ => Err(TrendRadarError::Config(format!("未知渠道: {}", channel))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markdown() {
        let input = "**粗体** 和 [链接](https://example.com) 普通文本";
        let result = strip_markdown(input);
        assert!(!result.contains("**"));
        assert!(!result.contains('['));
        assert!(!result.contains("]("));
        assert!(result.contains("链接"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn test_convert_markdown_to_mrkdwn() {
        let input = "**粗体** 和 [文本](https://example.com)";
        let result = convert_markdown_to_mrkdwn(input);
        assert!(result.contains("*粗体*"));
        assert!(result.contains("<https://example.com|文本>"));
    }

    #[test]
    fn test_get_batch_header() {
        let header = get_batch_header("feishu", 1, 3);
        assert!(header.contains("第 1/3 批次"));
        assert!(header.contains("**"));

        let header = get_batch_header("telegram", 2, 3);
        assert!(header.contains("<b>"));
    }
}
