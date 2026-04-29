# TrendRadar-rs

**全网热点聚合 · AI 深度分析 · 多层筛选 · 多渠推送**

用 Rust 完全重写的 TrendRadar。一个二进制文件搞定全部功能，不需要装 Python，不需要装 Docker。

> **声明：本项目代码、文档（包括本 README）均由 AI 辅助生成。项目目前处于早期阶段，功能和稳定性尚不完善，可能存在未知问题。欢迎提 Issue，但请保持耐心。**

> **致谢：本项目是基于 [TrendRadar](https://github.com/sansan0/TrendRadar) 的 Rust 移植版。感谢原作者 [sansan0](https://github.com/sansan0) 的杰出设计和开源精神。架构设计、调度模型、配置结构、通知渠道组合方案、AI 过滤与深度分析流水线等核心设计均继承自原项目。**

[![Rust](https://img.shields.io/badge/rust-1.95+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## 目录

- [它能做什么](#它能做什么)
  - [两种运行模式](#两种运行模式)
  - [AI 筛选容错机制](#ai-筛选容错机制)
- [准备工作（新手必读）](#准备工作新手必读)
- [1. 编译（在你的电脑上）](#1-编译在你的电脑上)
- [2. 配置（只改 3 处即可上线）](#2-配置只改-3-处即可上线)
  - [2.1 第一处：AI API Key](#21-第一处ai-api-key推荐用-env更安全)
  - [2.2 第二处：通知渠道](#22-第二处通知渠道必改至少选一个)
  - [2.3 第三处：调度模式](#23-第三处调度模式可选默认即可)
- [3. 首次运行验证](#3-首次运行验证)
- [4. 通知渠道配置详解](#4-通知渠道配置详解)
- [5. 时间线调度详解](#5-时间线调度详解)
- [6. 部署到 Linux 服务器](#6-部署到-linux-服务器)
- [7. 部署到 OpenWrt / 小米4A 路由器](#7-部署到-openwrt--小米4a-路由器)
- [8. MCP Server 模式](#8-mcp-server-模式)
- [9. 常见问题排查](#9-常见问题排查)
- [10. 目录结构](#10-目录结构)
- [11. 性能参考](#11-性能参考)
- [License](#license)

---

## 它能做什么

简单说：**自动帮你盯着全网的新闻热点，然后用 AI 分析，最后推送到你手机上。**

```
每次触发（由 crond 或 systemd 定时）
  │
  ├── 1. 抓取 11 个热榜平台的最新热搜（微博/百度/知乎/B站/抖音/头条...）
  ├── 2. 抓取你订阅的 RSS 源（博客/新闻网站...）
  ├── 3. 按关键词过滤 + AI 智能分类（多层容错，始终有结果）
  ├── 4. 发给 AI 做深度分析（趋势判断/舆情分析/投资信号）
  ├── 5. 生成漂亮的 HTML 网页报告 + Markdown
  └── 6. 推送到你手机（飞书/钉钉/企业微信/Telegram/邮件/Bark/ntfy...）
```

### 两种运行模式

| 模式 | 命令 | 适用场景 |
|------|------|---------|
| **一次性**（推荐） | `./trendradar --config config/config.yaml --once` | crond 定时触发，跑完退出 |
| **持续运行** | `./trendradar --config config/config.yaml` | systemd 守护，每 10 分钟爬取 |

> 持续运行模式：每 10 分钟爬一次热榜，**只有出现新新闻才调用 AI 和推送**，无新增则静默跳过。适合全天盯盘的场景。

### AI 筛选容错机制

AI 分类内置三级降级保障，确保始终有结果输出：

```
第 1 级：JSON 解析 → 成功 ✅
第 2 级：逐行 pipe 解析（ID|TAG|SCORE）→ 成功 ✅
第 3 级：frequency_words.txt 关键词匹配兜底 → 100% 可靠 ✅
```

> 每级失败自动重试 3 次（指数退避 1s/2s/4s），一个 chunk 失败不影响其他 chunk。

### 和原版 Python 的区别

| | 原版 Python | Rust 版 |
|---|---|---|
| 运行需要 | Python + pip + Docker | **一个二进制文件** |
| 启动速度 | 几秒（解释型） | 毫秒级（编译型） |
| 内存占用 | ~100 MB | **3-12 MB（峰值）** |
| 能跑在路由器上 | ❌ 太大 | ✅ 完全足够 |
| 依赖管理 | requirements.txt → 经常冲突 | 零外部依赖，全静态编译 |
| 配置管理 | config.yaml + 环境变量 | config.yaml + .env 文件 |

---

## 准备工作（新手必读）

如果你是第一次接触命令行，请先阅读本节。

### 安装 Rust（只需要做一次）

**Windows：**

1. 打开浏览器，访问 https://rustup.rs
2. 下载 `rustup-init.exe`，双击运行
3. 一路回车选默认选项，等待安装完成
4. 安装完成后，**关闭当前命令行窗口，重新打开一个新的**

验证安装：打开 PowerShell 或 CMD，输入：
```powershell
rustc --version
# 应该看到类似: rustc 1.95.0 (xxxxx)
cargo --version
# 应该看到类似: cargo 1.95.0 (xxxxx)
```

如果提示"找不到命令"，重启电脑再试。

**Linux：**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# 一路回车选默认安装，完成后执行：
source ~/.cargo/env
```

**macOS：**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 获取代码

```bash
git clone https://github.com/zeroasj/TrendRadar-rs.git
cd TrendRadar-rs
```

---

## 1. 编译（在你的电脑上）

> **关键理解：编译只需要在电脑上做一次，之后把编译好的二进制文件传到任何地方都能直接运行。路由器上不需要装 Rust。**

### 1.1 在你的电脑上编译

```bash
# 第一步：验证代码没有错误（约 1-3 分钟，首次需要下载依赖）
cargo check

# 第二步：编译 Release 版本（约 3-10 分钟，首次较慢）
cargo build --release
```

编译完成后，二进制文件在这里：
- **Windows**：`target\release\trendradar.exe`
- **Linux/macOS**：`target/release/trendradar`

文件大小约 7-12 MB（已配置了 LTO + strip 优化，最小化体积）。

### 1.2 交叉编译到其他平台

如果要部署到 MIPS 路由器（如小米 4A），需要在 x86 电脑上**交叉编译**。

**推荐方式：使用 Docker**

```bash
# Windows 用户：把 $(pwd) 换成完整路径，如 d:/trendradar-rs
# 编译 MIPS 版本（适用于 OpenWrt / 小米路由器）
# 首次编译：下载工具链 150-200MB，需等待几分钟；之后增量编译仅数十秒
docker run --rm \
  -v "$(pwd):/app" \
  -v cargo-cache:/root/.cargo \
  -v rustup-cache:/root/.rustup \
  -w /app \
  messense/rust-musl-cross:mipsel-musl \
  sh -c "rustup component add rust-src --toolchain nightly-2026-04-20 && \
         CC=mipsel-unknown-linux-musl-gcc \
         CFLAGS_mipsel_unknown_linux_musl='-march=mips32 -msoft-float' \
         RUSTFLAGS='-C target-cpu=mips32' \
         cargo +nightly-2026-04-20 build -Zbuild-std --target mipsel-unknown-linux-musl --release"

# 产物: target/mipsel-unknown-linux-musl/release/trendradar
```

> **关键：** `CFLAGS_mipsel_unknown_linux_musl` 控制 SQLite 的 C 代码编译指令集，必须和 `RUSTFLAGS` 一致（都用 `mips32` 以兼容软浮点 CPU）。

> **注意：** 不要使用 UPX 压缩——虽然能砍到 2.7MB（77%），但 UPX 在 MIPS musl 上不兼容，会导致进程启动失败。

编译其他架构只需换 Docker 镜像标签，参考 [rust-musl-cross](https://github.com/messense/rust-musl-cross) 项目。

### 1.3 二进制文件需要配套文件

编译出来的只是一个二进制文件，运行时需要同级目录下有这些文件：

```
趋势雷达/
├── trendradar          ← 编译出来的二进制
├── .env                ← 环境变量（API Key、邮箱密码等）
├── frequency_words.txt ← 关键词列表
├── ai_interests.txt    ← AI 兴趣描述
├── ai_analysis_prompt.txt
├── ai_translation_prompt.txt
├── config/
│   ├── config.yaml     ← 主配置文件
│   ├── timeline.yaml   ← 调度模板
│   └── ai_filter/      ← AI 筛选提示词
└── data/               ← 自动生成（数据库，可通过 storage.data_dir 配置到其他位置）
```

> **启动时可以通过 `--config` 指定配置文件的路径，程序会自动切换到配置文件所在的目录作为工作目录。** 比如 `./trendradar --config /opt/trendradar/config/config.yaml`，程序就会在 `/opt/trendradar/` 这个目录下找配置和数据文件。

> 以上所需文件都在仓库的 `config/` 目录下，部署时一起拷贝即可。参见 [第 6 章](#6-部署到-linux-服务器) 的打包步骤。

---

## 2. 配置（只改 3 处即可上线）

配置文件是 `config/config.yaml`，用任意文本编辑器（记事本/VSCode）打开。

> 完整配置文件很长，但**你只需要改 3 个地方**。其余全部用默认值就行。

### 2.1 第一处：AI API Key（推荐用 .env，更安全）

**方式一（推荐）：创建 `.env` 文件**

```bash
# 复制模板
cp .env.example .env

# 编辑 .env，填入你的真实密钥
```

`.env` 内容：
```ini
AI_API_KEY=sk-your-key-here
SMTP_PASSWORD=your-smtp-password     # 用于邮件通知，见第 4 章
```

**.env 文件已被 .gitignore 排除，不会上传到 GitHub。**

**方式二：直接在 config.yaml 中填写**

```yaml
ai:
  model: "deepseek/deepseek-v4-flash"  # 模型名
  api_key: "sk-xxxxxxxxxxxxxxxx"       # API Key
```

> 环境变量优先级高于 config.yaml。如果同时配置了 .env 和 config.yaml，以 .env 为准。

**怎么获取 API Key？**

| 模型商 | 注册地址 | 价格 | 推荐度 |
|--------|---------|------|--------|
| **DeepSeek** | https://platform.deepseek.com | 极便宜（约 0.1 元/天） | ⭐⭐⭐ |
| OpenAI | https://platform.openai.com | 较贵 | ⭐⭐ |
| SiliconFlow | https://siliconflow.cn | 免费额度多 | ⭐⭐⭐ |

> 暂时不想用 AI：把 `ai_analysis.enabled` 改成 `false`，把 `filter.method` 改成 `"keyword"`。完全免费。

### 2.2 第二处：通知渠道（必改，至少选一个）

打开 `config/config.yaml`，搜索 `notification:`（约在文件中间位置），把 `enabled` 改成 `true`，然后选一个渠道填上。**其他渠道留空就行，不会报错。**

> 详细获取方式见 [第 4 章：通知渠道配置详解](#4-通知渠道配置详解)

以最推荐的**邮件**为例（国内最通用）：

```yaml
notification:
  enabled: true
  channels:
    email:
      from: "你的QQ号@qq.com"
      password: ""                             # 留空，在 .env 中设置 SMTP_PASSWORD
      to: "接收者@qq.com"
```

### 2.3 第三处：调度模式（可选，默认即可）

```yaml
schedule:
  enabled: true
  preset: "morning_evening"   # 推荐的默认模式
```

5 种预设说明：

| 预设 | 效果 | 适合谁 |
|------|------|--------|
| `always_on` | 有新增就推，全天不停 | 重度用户 |
| `morning_evening` | 白天推送 current + **晚上推 daily 日报** | **大多数人（推荐）** |
| `office_hours` | 工作日：到岗/午间/下班各推一次 | 上班族 |
| `night_owl` | 午后一次 + 深夜汇总 | 夜猫子 |
| `custom` | 完全自定义（编辑 timeline.yaml） | 高级用户 |

---

## 3. 首次运行验证

配置文件改好后，先做 3 步验证：

### 3.1 诊断配置

```bash
./trendradar --doctor
```

这会检查你的配置文件是否有语法错误、AI 配置是否完整、通知渠道是否填写、当前时间段匹配情况等。

### 3.2 测试通知

```bash
# 测试通知（把 email 换成你配置的渠道名）
./trendradar --test-notification email
```

你应该能在手机上收到一条 "TrendRadar 测试消息"。

### 3.3 试运行一次

```bash
./trendradar --config config/config.yaml --once
```

观察命令行输出，正常的话会看到类似：

```
INFO trendradar: TrendRadar v0.1.0 启动中...
INFO trendradar: 配置加载成功
INFO trendradar: timeline 预设加载成功: morning_evening
INFO trendradar: [调度] 报告模式覆盖: current -> current
INFO trendradar: 开始新一轮数据采集 [模式: current]
INFO trendradar: 热榜爬取完成: 550 条
INFO trendradar: RSS 爬取完成: 120 条
INFO trendradar: 过滤结果: 热榜 85/550 条, RSS 12/120 条
INFO trendradar: 本轮完成，耗时 84s
```

### 3.4 正式启动（持续运行模式）

```bash
./trendradar --config config/config.yaml
```

> **推荐：** 用 crond 定时触发 `--once`，比持续运行更省资源。见 [第 7 章](#7-部署到-openwrt--小米4a-路由器)。

---

## 4. 通知渠道配置详解

### 4.1 邮件（🏆 最通用，无需额外 App）

**步骤（以 QQ 邮箱为例）：**

1. 登录 QQ 邮箱网页版 → 设置 → 账户
2. 往下找到"POP3/IMAP/SMTP/Exchange/CardDAV/CalDAV服务"
3. 开启"POP3/SMTP 服务"，会用短信验证
4. 验证后会显示一个 16 位授权码，**复制下来**（不是 QQ 密码！）
5. 创建 `.env` 文件或直接填入 config.yaml：

```yaml
    email:
      from: "你的QQ号@qq.com"
      password: ""            # 留空，在 .env 中配置 SMTP_PASSWORD
      to: "接收者@qq.com"
      smtp_server: ""        # 留空自动识别（QQ邮箱自动用 smtp.qq.com）
      smtp_port: ""          # 留空自动识别
```

```ini
# .env
SMTP_PASSWORD=刚复制的16位授权码
```

### 4.2 飞书（国内直连，配置简单）

1. 飞书 → 进入一个群聊（可以建一个只有自己的群）
2. 群设置 → 群机器人 → 添加机器人 → 自定义机器人
3. 复制 Webhook URL

```yaml
    feishu:
      webhook_url: "https://open.feishu.cn/open-apis/bot/v2/hook/xxxxxxxxx"
```

### 4.3 钉钉

1. 钉钉 → 进入一个群
2. 群设置 → 智能群助手 → 添加机器人 → 自定义（通过 Webhook 接入）
3. 复制 Webhook URL

```yaml
    dingtalk:
      webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=xxxxxxxxx"
```

### 4.4 企业微信

```yaml
    wework:
      webhook_url: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=xxxxxxxxx"
```

### 4.5 Bark（仅 iPhone/iPad）

1. App Store 安装 Bark
2. 复制首页显示的推送链接

```yaml
    bark:
      url: "https://api.day.app/xxxxxx"
```

### 4.6 Telegram（需翻墙）

1. 搜索 `@BotFather`，发送 `/newbot` 创建机器人
2. 搜索 `@userinfobot` 获取 chat_id

```yaml
    telegram:
      bot_token: "1234567890:ABCdefGHijklmnOPQRSTuvwxyz"
      chat_id: "987654321"
```

### 4.7 其他渠道

参考 config.yaml 中 `notification.channels` 段注释，支持 ntfy、Slack、通用 Webhook 等。

---

## 5. 时间线调度详解

### 5.1 工作原理

程序不是 24 小时不间断推送，那样半夜也会吵你。而是按你设定的**时间段**来工作。

```
morning_evening 模式的一天：
  00:00 ────────────────────────────────────────── 23:59
    │                                          │
    │  默认时间段（全天）                       │  晚间汇总 (20:00-22:00)
    │  report_mode: current                    │  report_mode: daily
    │  推送当前在榜热点                        │  AI 分析全天数据
    │                                          │  推送完整日报
```

### 5.2 配置位置

两个文件配合：

- `config/config.yaml` → `schedule.preset` 选模板
- `config/timeline.yaml` → 模板的具体定义（预设已配好，一般不动它）

**Rust 版完整支持 timeline.yaml，和 Python 原版行为一致。**

### 5.3 各预设详解

#### morning_evening（推荐）

```
默认（全天）：report_mode = current（当前热点）
晚间汇总 (20:00-22:00)：report_mode = daily（全天日报）
```

白天每次触发推送的是"当前在榜"热点；晚上 8-10 点之间触发时自动切换为 daily 模式，推送全天汇总。

#### office_hours（上班族）

```
默认（静默期）：采集但不推送
09:00-11:00 到岗速览：current
13:00-15:00 午间热点：current
17:00-19:00 收工汇总：daily
周末全天：incremental
```

#### night_owl（夜猫子）

```
默认（白天静默）：采集但不推送
15:00-17:00 午后速览：current
22:00-01:00 深夜汇总：daily（跨日支持）
```

#### always_on（全天候）

```
全天：report_mode = incremental（有新增就推，无打扰）
不划分时间段，全天同一套配置
```

### 5.4 自定义时间段

把 `preset` 改成 `"custom"`，然后编辑 `timeline.yaml` 底部的 `custom` 段。

核心概念：

```yaml
# 第 1 步：定义"时间段积木"
periods:
  morning:
    name: "早间推送"
    start: "08:00"
    end: "10:00"
    push: true
    report_mode: "current"

  evening:
    name: "晚间汇总"
    start: "20:00"
    end: "22:00"
    push: true
    analyze: true
    report_mode: "daily"

# 第 2 步：把积木拼成"一天的计划"
day_plans:
  workday:
    periods: ["morning", "evening"]
  lazy_day:
    periods: ["evening"]

# 第 3 步：指定周几用哪个计划
week_map:
  1: "workday"    # 周一
  ...
  6: "lazy_day"   # 周六
  7: "lazy_day"   # 周日
```

支持跨日时间段（如 `start: "22:00"` `end: "01:00"` 自动识别为跨午夜）。

---

## 6. 部署到 Linux 服务器

> 假设你有一台 Linux 服务器（VPS/树莓派/旧电脑/NAS），要在上面长期运行。

### 6.1 打包上传

**在你的电脑上执行：**

```bash
# 1. 编译
cargo build --release

# 2. 打包配置和二进制
mkdir -p deploy/config
cp target/release/trendradar deploy/
cp -r config/* deploy/config/
cp .env deploy/                             # 如果有的话
tar czf trendradar-deploy.tar.gz deploy/

# 3. 上传到服务器
scp trendradar-deploy.tar.gz root@你的服务器IP:/opt/
```

**在服务器上执行：**

```bash
cd /opt
tar xzf trendradar-deploy.tar.gz
mv deploy trendradar
cd trendradar

# 编辑配置
nano config/config.yaml

# 测试
chmod +x trendradar
./trendradar --doctor
./trendradar --once
```

### 6.2 设为开机自启（systemd）

```bash
sudo useradd -r -s /bin/false trendradar
sudo chown -R trendradar:trendradar /opt/trendradar

sudo nano /etc/systemd/system/trendradar.service
```

```ini
[Unit]
Description=TrendRadar 热点聚合分析
After=network-online.target

[Service]
Type=simple
User=trendradar
WorkingDirectory=/opt/trendradar
ExecStart=/opt/trendradar/trendradar
Restart=always
RestartSec=30

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now trendradar
sudo systemctl status trendradar
```

---

## 7. 部署到 OpenWrt / 小米4A 路由器

> 实测：小米 4A 千兆版路由器（MIPS 32-bit，16MB Flash，128MB RAM，OpenWrt 25.12）稳定运行，内存峰值 ~12MB，二进制 ~11MB。

### 7.1 交叉编译

见 [1.2 节](#12-交叉编译到其他平台) 的命令。编译完成后：

```bash
cp target/mipsel-unknown-linux-musl/release/trendradar deploy/
```

### 7.2 部署到路由器

```bash
# 上传整个 deploy 目录（含 config、frequency_words.txt 等）
scp -O -r deploy root@192.168.1.1:/root/trendradar
```

### 7.3 首次配置

```bash
# SSH 进入路由器
ssh root@192.168.1.1

# 修复 MIPS 软浮点链接器兼容（首次部署必须，写入 rc.local 开机自动恢复）
ln -s /lib/ld-musl-mipsel-sf.so.1 /lib/ld-musl-mipsel.so.1
echo "ln -s /lib/ld-musl-mipsel-sf.so.1 /lib/ld-musl-mipsel.so.1" >> /etc/rc.local

# 创建 .env
cd /root/trendradar
cat > .env << 'EOF'
AI_API_KEY=sk-your-key-here
SMTP_PASSWORD=your-smtp-password
EOF

# 试运行
/root/trendradar/trendradar --config /root/trendradar/config/config.yaml --once
```

### 7.4 定时任务（crontab）

```bash
# 每 4 小时执行一次（分钟偏移 3，避免整点冲突）
echo "3 */4 * * * /root/trendradar/trendradar --config /root/trendradar/config/config.yaml --once >> /tmp/trendradar.log 2>&1" | crontab -

# 验证
crontab -l
```

> `--once` 表示执行一轮后退出。不加 `--once` 会进入持续运行的调度循环模式（适合 systemd 部署，不适合 crond）。

> 自 v0.1.0 起，程序通过 `app.timezone` 自行管理时区，报告和日志时间自动使用本地时间，无需额外配置。

### 7.5 存储空间管理

小米 4A 的 overlay 分区仅 6.7MB。如果空间紧张：

**方案一：数据库放到 tmpfs**

编辑 `config/config.yaml`，在 `storage` 段加一行：
```yaml
storage:
  backend: "auto"
  data_dir: "/tmp/trendradar-data"    # 空间充裕（58MB），重启后自动重建
```

> 重启后数据库丢失，程序自动建新库从头采集。

**方案二：挂载局域网存储**

```bash
# 挂载 NAS/PC 共享目录
mkdir -p /mnt/nas
mount -t nfs 192.168.1.xx:/share /mnt/nas
```

然后设 `data_dir: "/mnt/nas"`，数据库永久保存在 NAS 上。

### 7.6 更新二进制

以后改了代码，只需要重新交叉编译并上传二进制（配置不变）：

```bash
# 在电脑上编译
docker run --rm -v "d:\.../trendradar-rs:/app" ... cargo build ...

# 上传并替换
scp -O deploy/trendradar root@192.168.1.1:/root/trendradar/
```
---


## 8. MCP Server 模式

MCP (Model Context Protocol) 可以让 AI 客户端（Claude Desktop / Cursor 等）直接查询你的热点数据库。共提供 27 个工具，覆盖查询、分析、报告、操作等场景。

### 8.1 启动

```bash
# 前台运行（测试用）
./trendradar serve --port 8080

# 后台运行（生产用）
nohup ./trendradar serve --port 8080 >> /tmp/trendradar-mcp.log 2>&1 &

# 开机自启（OpenWrt）
echo "/root/trendradar/trendradar serve --port 8080 >> /tmp/trendradar-mcp.log 2>&1 &" >> /etc/rc.local
```

> 内存占用约 15-18 MB（常驻），90% 与 crond 定时任务无关——`--once` 跑完就释放内存，MCP Server 是独立进程。两者可以共存。

### 8.2 连接到 Claude Desktop

编辑 Claude Desktop 配置文件：
- **Windows**：`%APPDATA%\Claude\claude_desktop_config.json`
- **macOS**：`~/Library/Application Support/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "trendradar": {
      "command": "路径/到/trendradar",
      "args": ["serve", "--port", "8080"]
    }
  }
}
```

### 8.3 可用的 MCP 工具（27 个）

| 工具名 | 功能 | 分类 |
|--------|------|------|
| `get_latest_news` | 获取最新一批热榜新闻 | 查询 |
| `get_latest_rss` | 获取最新 RSS 订阅数据 | 查询 |
| `get_news_by_date` | 按日期查询历史热榜 | 查询 |
| `search_news` | 按关键词搜索热榜新闻（支持热榜+RSS） | 查询 |
| `search_rss` | 按关键词搜索 RSS 数据 | 查询 |
| `find_related_news` | 根据标题查找相关新闻 | 查询 |
| `list_available_dates` | 列出本地可用的数据日期范围 | 查询 |
| `get_trending_topics` | 热门话题频率统计（预置/自动提取） | 分析 |
| `analyze_topic_trend` | 话题趋势分析（热度/生命周期/预测） | 分析 |
| `analyze_data_insights` | 数据洞察（平台比较/活跃度/关键词共现） | 分析 |
| `analyze_sentiment` | 新闻情感极性分析（正/负/中性） | 分析 |
| `aggregate_news` | 跨平台新闻去重合并 | 分析 |
| `compare_periods` | 两个时间段的新闻差异对比 | 分析 |
| `generate_summary_report` | 生成日/周摘要报告（Markdown） | 报告 |
| `resolve_date_range` | 自然语言日期解析（"今天""本周"等） | 工具 |
| `get_current_config` | 查看当前系统配置（按分类） | 工具 |
| `get_system_status` | 系统状态和健康检查 | 工具 |
| `check_version` | 检查版本更新 | 工具 |
| `get_rss_feeds_status` | RSS 源状态和数据统计 | 工具 |
| `get_storage_status` | 存储配置和后端状态 | 工具 |
| `get_notification_channels` | 已配置的通知渠道及状态 | 工具 |
| `get_channel_format_guide` | 各渠道格式限制说明（9 种） | 工具 |
| `trigger_crawl` | 手动触发一次抓取 | 操作 |
| `sync_from_remote` | 从远程存储拉取数据 | 操作 |
| `send_notification` | 发送消息到通知渠道（自动适配格式） | 操作 |
| `read_article` | 通过 Jina AI Reader 读取文章内容 | 操作 |
| `read_articles_batch` | 批量读取多篇文章（最多 5 篇） | 操作 |

---

## 9. 常见问题排查

### Q: `cargo check` 报错"could not compile ..."

**A:** 最常见的原因是缺少 C 编译器（用于编译 SQLite）。

- **Windows**：安装 [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- **Linux**：`sudo apt install build-essential`
- **macOS**：`xcode-select --install`

### Q: 通知测试收不到消息

**A:** 按顺序排查：
1. `notification.enabled` 是不是 `true`？
2. 对应渠道的配置是否填写了？
3. 网络是否正常？路由器能访问外网吗？
4. Webhook URL 有没有写错？

### Q: AI 分析报错或没反应

**A:**
1. `AI_API_KEY` 环境变量或 `.env` 文件是否配置？
2. `ai_analysis.enabled` 是否为 `true`？
3. 网络能不能访问 AI API？
4. 用 `--doctor` 诊断配置
5. 检查 timeline 调度：当前时间段是否允许 analyze？

### Q: 路由器上提示 "Illegal instruction"

**A:** 交叉编译时未同时控制 C 编译器指令集，导致 SQLite 的 C 代码生成了不兼容的指令。请用 README 中的完整编译命令重新编译，确保同时设置 `CFLAGS_mipsel_unknown_linux_musl` 和 `RUSTFLAGS`（都用 `mips32`）。

### Q: 路由器上提示 "not found"

**A:** MIPS 动态链接器文件名不匹配。执行：
```bash
ln -s /lib/ld-musl-mipsel-sf.so.1 /lib/ld-musl-mipsel.so.1
echo "ln -s /lib/ld-musl-mipsel-sf.so.1 /lib/ld-musl-mipsel.so.1" >> /etc/rc.local
```
详见 [7.3 节](#73-首次配置)。

### Q: 怎么完全不花钱运行？

**A:**
1. `filter.method` 改用 `"keyword"`（不调用 AI 筛选）
2. `ai_analysis.enabled` 改为 `false`
3. `ai_translation.enabled` 改为 `false`
4. 用 crond 定时 `--once` 触发（省电、省资源）
5. 通知用邮件/飞书（免费）

### Q: 为什么不推送到我了？

**A:** 检查 timeline 调度日志：
```
[调度] 当前时间段: 默认配置（未命中任何时间段）
[调度] 行为: 采集, 分析(AI:current), 推送(模式:current)
```
如果 `推送` 没出现，说明当前时间段 push=false。编辑 `timeline.yaml` 调整。

---

## 10. 目录结构

```
TrendRadar-rs/
│
├── Cargo.toml              ← Rust 项目配置 + 依赖
├── Cargo.lock              ← 依赖版本锁定
├── .env.example            ← 环境变量模板（可安全上传）
├── .gitignore              ← Git 排除规则
├── README.md               ← 你正在看的文件
│
├── src/
│   └── main.rs             ← 程序入口（CLI + 主流程）
│
├── crates/
│   ├── core/src/           ← 核心功能库
│   │   ├── config.rs       ← YAML 配置解析 + .env 覆盖
│   │   ├── model.rs        ← 数据模型
│   │   ├── error.rs        ← 错误类型定义
│   │   ├── storage.rs      ← SQLite 数据库操作
│   │   ├── crawler.rs      ← 网络爬虫（11 平台热榜 + RSS）
│   │   ├── matcher.rs      ← 关键词匹配引擎
│   │   ├── notify.rs       ← 9 种通知渠道
│   │   ├── ai.rs           ← AI 客户端（分析/筛选/翻译）
│   │   ├── report.rs       ← Markdown/RSS 报告生成
│   │   ├── templates.rs    ← HTML 模板渲染（Askama）
│   │   ├── scheduler.rs    ← 时间线调度器
│   │   ├── timeline.rs     ← timeline.yaml 解析器
│   │   └── lib.rs          ← 模块导出
│   │
│   ├── platform/src/       ← MCP Server
│   │   ├── mcp.rs          ← JSON-RPC 2.0 协议 + 27 个工具
│   │   └── lib.rs
│   │
│   └── embedded/           ← ESP32 占位
│
├── config/                 ← 配置文件（部署时一起拷贝）
│   ├── config.yaml         ← 主配置（全中文注释）
│   ├── config.example.yaml ← 配置模板（可安全上传）
│   ├── timeline.yaml       ← 调度模板定义（支持 4 种预设 + 自定义）
│   ├── frequency_words.txt ← 关键词列表
│   ├── ai_interests.txt    ← AI 兴趣描述
│   ├── ai_analysis_prompt.txt
│   ├── ai_translation_prompt.txt
│   └── ai_filter/          ← AI 筛选提示词
│
├── crates/core/templates/  ← HTML 报告模板
│   ├── base.html           ← 基础布局
│   └── report.html         ← 报告模板
│
└── output/                 ← 运行后自动生成（数据库/报告）
```

---

## 11. 性能参考

### 不同部署方式的资源消耗

| | 二进制大小 | 内存（高峰） | CPU |
|---|---|---|---|
| **x86_64 Linux** | ~10 MB | ~30 MB | <1% |
| **树莓派 4** | ~10 MB | ~35 MB | 5-15% |
| **小米 4A 路由器** (MIPS) | ~11 MB | ~12 MB | 10-30% |
| **小米 4A + MCP Server 常驻** | ~11 MB | ~15-18 MB | 5-15%（空闲） |

### 单轮采集数据量

| 项目 | 数量 |
|------|------|
| 热榜新闻 | ~500-600 条/轮（11 个平台） |
| RSS 文章 | ~50-500 条/轮 |
| 网络流量 | ~1-5 MB/轮 |
| AI API 调用 | 0-3 次/轮 |
| AI 费用 | ~0.001-0.02 元/轮（DeepSeek） |

### 数据库增长

| 运行时间 | 数据库大小 |
|----------|-----------|
| 1 天 | ~1-3 MB |
| 1 周 | ~5-15 MB |
| 1 个月 | ~20-50 MB |

---

## 环境变量

敏感信息通过 `.env` 文件管理（程序启动时自动加载），不写入 config.yaml，更安全：

| 变量 | 说明 | 优先级 |
|------|------|--------|
| `AI_API_KEY` | 通用 AI API Key（所有 AI 模块默认值） | 覆盖 config.yaml |
| `AI_FILTER_API_KEY` | AI 筛选专用 Key | > AI_API_KEY |
| `AI_ANALYSIS_API_KEY` | AI 分析专用 Key | > AI_API_KEY |
| `AI_TRANSLATION_API_KEY` | AI 翻译专用 Key | > AI_API_KEY |
| `SMTP_PASSWORD` | 邮件 SMTP 密码/授权码 | 覆盖 config.yaml |

环境变量优先级：指定模块 Key > `AI_API_KEY` > config.yaml 中的值。

### 时区配置

在 `config/config.yaml` 中通过 `app.timezone` 设置：

```yaml
app:
  timezone: "CST-8"   # 北京时间 UTC+8
```

程序启动时自动设置 `TZ` 环境变量，之后所有日志、报告、数据库存储均使用该时区。

> **如果用了 glibc（如普通 Linux 服务器），** 也可以用 IANA 名称（如 `Asia/Shanghai`），前提是系统安装了 zoneinfo 数据。
>
> **如果用了 musl（如 OpenWrt / Alpine / 容器精简镜像 / 交叉编译的 MIPS 版本），** 使用 POSIX 格式（如 `CST-8`），无需 zoneinfo 文件即可正常工作。

---
## License

MIT — 与 [原版 TrendRadar](https://github.com/sansan0/TrendRadar) 保持一致。
