use chrono::{DateTime, Datelike, Local, Timelike};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct TimelineFile {
    pub presets: HashMap<String, TimelinePreset>,
    #[serde(default)]
    pub custom: Option<TimelinePreset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimelinePreset {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub default: TimelineDefaults,
    #[serde(default)]
    pub periods: HashMap<String, TimelinePeriod>,
    pub day_plans: HashMap<String, DayPlan>,
    pub week_map: HashMap<i32, String>,
    #[serde(default)]
    pub overlap: Option<OverlapPolicy>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimelineDefaults {
    #[serde(default = "default_true")]
    pub collect: bool,
    #[serde(default)]
    pub analyze: bool,
    #[serde(default)]
    pub ai_mode: Option<String>,
    #[serde(default)]
    pub push: bool,
    #[serde(default = "default_report_mode")]
    pub report_mode: String,
    #[serde(default)]
    pub once: OnceConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OnceConfig {
    #[serde(default)]
    pub analyze: bool,
    #[serde(default)]
    pub push: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimelinePeriod {
    #[serde(default)]
    pub name: Option<String>,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub collect: Option<bool>,
    #[serde(default)]
    pub analyze: Option<bool>,
    #[serde(default)]
    pub ai_mode: Option<String>,
    #[serde(default)]
    pub push: Option<bool>,
    #[serde(default)]
    pub report_mode: Option<String>,
    #[serde(default)]
    pub once: Option<OnceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DayPlan {
    pub periods: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverlapPolicy {
    #[serde(default)]
    pub policy: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_report_mode() -> String {
    "current".to_string()
}

#[derive(Debug, Clone)]
pub struct ResolvedTimeline {
    pub period_key: Option<String>,
    pub period_name: Option<String>,
    pub day_plan: String,
    pub collect: bool,
    pub analyze: bool,
    pub push: bool,
    pub report_mode: String,
    pub ai_mode: String,
    pub once_analyze: bool,
    pub once_push: bool,
}

pub struct TimelineResolver {
    preset: TimelinePreset,
}

impl TimelineResolver {
    pub fn from_preset_name(timeline: &TimelineFile, preset_name: &str) -> Option<Self> {
        if preset_name == "custom" {
            timeline.custom.as_ref().map(|p| Self { preset: p.clone() })
        } else {
            timeline.presets.get(preset_name).map(|p| Self { preset: p.clone() })
        }
    }

    pub fn resolve(&self, now: &DateTime<Local>) -> ResolvedTimeline {
        let default = &self.preset.default;
        let weekday = Self::local_weekday(now);
        let now_hhmm = format!("{:02}:{:02}", now.hour(), now.minute());

        let day_plan_key = self
            .preset
            .week_map
            .get(&weekday)
            .cloned()
            .unwrap_or_else(|| "all_day".to_string());
        let day_plan = self.preset.day_plans.get(&day_plan_key);

        let period_key = day_plan
            .and_then(|dp| self.find_active_period(&now_hhmm, &dp.periods));

        let (collect, analyze, push, report_mode, ai_mode, once_analyze, once_push) =
            self.merge_with_default(period_key.as_deref(), default);

        let period_name = period_key
            .as_ref()
            .and_then(|k| self.preset.periods.get(k))
            .and_then(|p| p.name.clone());

        ResolvedTimeline {
            period_key,
            period_name,
            day_plan: day_plan_key,
            collect,
            analyze,
            push,
            report_mode,
            ai_mode,
            once_analyze,
            once_push,
        }
    }

    fn local_weekday(now: &DateTime<Local>) -> i32 {
        match now.weekday() {
            chrono::Weekday::Mon => 1,
            chrono::Weekday::Tue => 2,
            chrono::Weekday::Wed => 3,
            chrono::Weekday::Thu => 4,
            chrono::Weekday::Fri => 5,
            chrono::Weekday::Sat => 6,
            chrono::Weekday::Sun => 7,
        }
    }

    fn find_active_period(&self, now_hhmm: &str, period_keys: &[String]) -> Option<String> {
        let mut candidates: Vec<(usize, String)> = Vec::new();
        for (idx, key) in period_keys.iter().enumerate() {
            if let Some(period) = self.preset.periods.get(key) {
                if Self::in_range(now_hhmm, &period.start, &period.end) {
                    candidates.push((idx, key.clone()));
                }
            }
        }

        if candidates.is_empty() {
            return None;
        }

        if candidates.len() > 1 {
            let policy = self
                .preset
                .overlap
                .as_ref()
                .and_then(|o| o.policy.as_deref())
                .unwrap_or("error_on_overlap");
            if policy == "error_on_overlap" {
                let names: Vec<String> = candidates.iter().map(|c| c.1.clone()).collect();
                tracing::warn!(
                    "时间段重叠冲突: {} 在 {} 重叠，使用 period 列表中最后匹配的",
                    names.join(", "),
                    now_hhmm
                );
            }
            return Some(candidates.last().unwrap().1.clone());
        }

        Some(candidates[0].1.clone())
    }

    fn in_range(now_hhmm: &str, start: &str, end: &str) -> bool {
        if start <= end {
            start <= now_hhmm && now_hhmm < end
        } else {
            now_hhmm >= start || now_hhmm < end
        }
    }

    fn merge_with_default(
        &self,
        period_key: Option<&str>,
        default: &TimelineDefaults,
    ) -> (bool, bool, bool, String, String, bool, bool) {
        let mut collect = default.collect;
        let mut analyze = default.analyze;
        let mut push = default.push;
        let mut report_mode = default.report_mode.clone();
        let mut ai_mode = default.ai_mode.clone();
        let mut once_analyze = default.once.analyze;
        let mut once_push = default.once.push;

        if let Some(key) = period_key {
            if let Some(period) = self.preset.periods.get(key) {
                if let Some(v) = period.collect { collect = v; }
                if let Some(v) = period.analyze { analyze = v; }
                if let Some(v) = period.push { push = v; }
                if let Some(ref v) = period.report_mode { report_mode = v.clone(); }
                if let Some(ref v) = period.ai_mode { ai_mode = Some(v.clone()); }
                if let Some(ref once) = period.once {
                    once_analyze = once.analyze;
                    once_push = once.push;
                }
            }
        }

        let ai_mode = match ai_mode.as_deref() {
            Some("follow_report") | None => report_mode.clone(),
            Some(other) => other.to_string(),
        };

        (collect, analyze, push, report_mode, ai_mode, once_analyze, once_push)
    }
}

impl TimelineFile {
    pub fn load(path: &str) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::error::TrendRadarError::Config(format!("读取 timeline.yaml 失败: {}", e)))?;
        let timeline: Self = serde_yaml::from_str(&content)
            .map_err(|e| crate::error::TrendRadarError::Config(format!("解析 timeline.yaml 失败: {}", e)))?;
        Ok(timeline)
    }
}
