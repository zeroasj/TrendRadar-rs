use crate::config::{AppConfig, SchedulePeriod};
use crate::timeline::{ResolvedTimeline, TimelineFile, TimelineResolver};
use chrono::{DateTime, Datelike, Local, Timelike, Utc, Weekday};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ScheduledRun {
    pub period_id: String,
    pub run_time: DateTime<Utc>,
    pub is_missed: bool,
}

pub struct TimelineScheduler {
    config: Arc<AppConfig>,
    last_runs: Arc<Mutex<HashSet<String>>>,
    timeline_resolver: Option<TimelineResolver>,
    schedule_enabled: bool,
    fallback_report_mode: String,
}

impl TimelineScheduler {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let schedule_enabled = config.schedule.as_ref()
            .and_then(|s| s.enabled)
            .unwrap_or(false);
        let preset_name = config.schedule.as_ref()
            .and_then(|s| s.preset.clone())
            .unwrap_or_else(|| "always_on".to_string());
        let fallback_report_mode = config.report.as_ref()
            .and_then(|r| r.mode.clone())
            .unwrap_or_else(|| "current".to_string());

        let timeline_resolver = if schedule_enabled {
            match TimelineFile::load("timeline.yaml") {
                Ok(timeline) => {
                    match TimelineResolver::from_preset_name(&timeline, &preset_name) {
                        Some(resolver) => {
                            tracing::info!("timeline 预设加载成功: {}", preset_name);
                            Some(resolver)
                        }
                        None => {
                            tracing::warn!("timeline 预设 '{}' 不存在，回退到默认行为", preset_name);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("无法加载 timeline.yaml: {}，回退到默认行为", e);
                    None
                }
            }
        } else {
            None
        };

        TimelineScheduler {
            config,
            last_runs: Arc::new(Mutex::new(HashSet::new())),
            timeline_resolver,
            schedule_enabled,
            fallback_report_mode,
        }
    }

    pub fn resolve(&self) -> ResolvedTimeline {
        if !self.schedule_enabled || self.timeline_resolver.is_none() {
            return ResolvedTimeline {
                period_key: None,
                period_name: None,
                day_plan: "disabled".to_string(),
                collect: true,
                analyze: true,
                push: true,
                report_mode: self.fallback_report_mode.clone(),
                ai_mode: "follow_report".to_string(),
                once_analyze: false,
                once_push: false,
            };
        }

        let now = Local::now();
        let resolved = self.timeline_resolver.as_ref().unwrap().resolve(&now);

        let weekday_names = ["", "一", "二", "三", "四", "五", "六", "日"];
        let weekday = match now.weekday() {
            Weekday::Mon => 1, Weekday::Tue => 2, Weekday::Wed => 3,
            Weekday::Thu => 4, Weekday::Fri => 5, Weekday::Sat => 6, Weekday::Sun => 7,
        };
        let period_display = match (&resolved.period_key, &resolved.period_name) {
            (Some(k), Some(n)) => format!("{} ({})", n, k),
            (Some(k), None) => k.clone(),
            _ => "默认配置（未命中任何时间段）".to_string(),
        };

        tracing::info!("[调度] 星期{}，日计划: {}", weekday_names[weekday as usize], resolved.day_plan);
        tracing::info!("[调度] 当前时间段: {}", period_display);

        let mut actions: Vec<String> = Vec::new();
        if resolved.collect { actions.push("采集".to_string()); }
        if resolved.analyze { actions.push(format!("分析(AI:{})", resolved.ai_mode)); }
        if resolved.push { actions.push(format!("推送(模式:{})", resolved.report_mode)); }
        tracing::info!("[调度] 行为: {}", if actions.is_empty() { "无".to_string() } else { actions.join(", ") });

        resolved
    }

    pub fn get_periods(&self) -> &[SchedulePeriod] {
        self.config.schedule.as_ref().map(|s| s.periods.as_slice()).unwrap_or(&[])
    }

    pub fn should_run(&self, period: &SchedulePeriod, current: DateTime<Local>) -> bool {
        if !period.enabled {
            return false;
        }

        let day_matches = if let Some(ref run_days) = period.run_days {
            let weekday = match current.weekday() {
                Weekday::Mon => 1,
                Weekday::Tue => 2,
                Weekday::Wed => 3,
                Weekday::Thu => 4,
                Weekday::Fri => 5,
                Weekday::Sat => 6,
                Weekday::Sun => 7,
            };
            run_days.contains(&weekday)
        } else {
            true
        };

        if !day_matches {
            return false;
        }

        let run_hours = match &period.run_hours {
            Some(h) if !h.is_empty() => h.clone(),
            _ => return true,
        };

        let current_hour = current.hour();
        let current_minute = current.minute();

        for &hour in &run_hours {
            let minute = period.run_minutes.as_ref()
                .and_then(|m| m.iter().find(|&&mm| mm / 60 == 0).copied())
                .unwrap_or(0);
            if current_hour == hour && current_minute >= minute && current_minute <= minute + 1 {
                return true;
            }
        }

        false
    }

    pub async fn get_due_periods(&self) -> Vec<SchedulePeriod> {
        let now = Local::now();
        let last_runs = self.last_runs.lock().await;

        let periods = match &self.config.schedule {
            Some(s) => &s.periods,
            None => return vec![],
        };
        periods.iter()
            .filter(|p| {
                if !p.enabled {
                    return false;
                }
                let key = format!("{}:{}", p.id, now.format("%Y%m%d%H"));
                if last_runs.contains(&key) {
                    return false;
                }
                self.should_run(p, now)
            })
            .cloned()
            .collect()
    }

    pub async fn mark_run(&self, period_id: &str) {
        let now = Local::now();
        let key = format!("{}:{}", period_id, now.format("%Y%m%d%H"));
        self.last_runs.lock().await.insert(key);
    }

    pub async fn clear_old_runs(&self) {
        let now = Local::now();
        let today_prefix = now.format("%Y%m%d").to_string();
        self.last_runs
            .lock()
            .await
            .retain(|k| k.contains(&today_prefix) || k.starts_with("manual:"));
    }

    pub fn is_period_active(&self, period: &SchedulePeriod, current: DateTime<Local>) -> bool {
        if !period.enabled {
            return false;
        }

        if let Some(ref run_days) = period.run_days {
            let weekday = match current.weekday() {
                Weekday::Mon => 1,
                Weekday::Tue => 2,
                Weekday::Wed => 3,
                Weekday::Thu => 4,
                Weekday::Fri => 5,
                Weekday::Sat => 6,
                Weekday::Sun => 7,
            };
            if !run_days.contains(&weekday) {
                return false;
            }
        }

        let run_hours = match &period.run_hours {
            Some(h) if !h.is_empty() => h.clone(),
            _ => return true,
        };

        let current_hour = current.hour();
        for &hour in &run_hours {
            if current_hour == hour && current.minute() < 2 {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn make_test_period(id: &str, enabled: bool) -> SchedulePeriod {
        SchedulePeriod {
            id: id.to_string(),
            name: Some(format!("Test {}", id)),
            enabled,
            cron: None,
            run_days: None,
            run_hours: Some(vec![0]),
            run_minutes: None,
            collect: None,
            analyze: None,
            push: None,
            report_mode: None,
            platforms: Vec::new(),
        }
    }

    #[test]
    fn test_should_run_disabled() {
        let period = make_test_period("test", false);
        let scheduler = TimelineScheduler::new(Arc::new(AppConfig::default()));
        assert!(!scheduler.should_run(&period, Local::now()));
    }

    #[test]
    fn test_should_run_enabled() {
        let period = make_test_period("test", true);
        let scheduler = TimelineScheduler::new(Arc::new(AppConfig::default()));
        let now = Local::now();
        let has_hour_match = period
            .run_hours
            .as_ref()
            .map(|h| h.contains(&now.hour()))
            .unwrap_or(true);
        if has_hour_match && now.minute() < 1 {
            assert!(scheduler.should_run(&period, now));
        }
    }
}
