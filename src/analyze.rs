use crate::config::Config;
use crate::scan::ScanResult;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Alert,
    Unused,
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Alert => "alert",
            Status::Unused => "unused",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerReport {
    pub server: String,
    pub status: Status,
    pub calls_total: u64,
    pub calls_30d: u64,
    pub calls_14d: u64,
    pub calls_7d: u64,
    pub last_call: Option<DateTime<Utc>>,
    pub days_since_last: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub scanned_at: DateTime<Utc>,
    pub transcripts_scanned: usize,
    pub warn_days: i64,
    pub alert_days: i64,
    pub servers: Vec<ServerReport>,
}

pub fn build(scan: ScanResult, cfg: &Config) -> Result<Report> {
    let now = scan.scanned_at;
    let mut servers: Vec<ServerReport> = scan
        .servers
        .into_values()
        .map(|s| {
            let days_since_last = s.last_call.map(|t| (now - t).num_days());
            let status = classify(s.calls_total, days_since_last, cfg);
            ServerReport {
                server: s.server,
                status,
                calls_total: s.calls_total,
                calls_30d: s.calls_30d,
                calls_14d: s.calls_14d,
                calls_7d: s.calls_7d,
                last_call: s.last_call,
                days_since_last,
            }
        })
        .collect();
    servers.sort_by(|a, b| {
        b.calls_30d
            .cmp(&a.calls_30d)
            .then_with(|| a.server.cmp(&b.server))
    });

    Ok(Report {
        scanned_at: now,
        transcripts_scanned: scan.transcripts_scanned,
        warn_days: cfg.warn_days,
        alert_days: cfg.alert_days,
        servers,
    })
}

fn classify(total: u64, days_idle: Option<i64>, cfg: &Config) -> Status {
    match days_idle {
        None => {
            if total == 0 {
                Status::Unused
            } else {
                Status::Alert
            }
        }
        // Future-dated timestamps (clock skew) clamp to "just called" rather
        // than wrap into Alert.
        Some(d) => {
            let d = d.max(0);
            if d >= cfg.alert_days {
                Status::Alert
            } else if d >= cfg.warn_days {
                Status::Warn
            } else {
                Status::Ok
            }
        }
    }
}

pub fn print_table(report: &Report) {
    println!(
        "mcp-prune — scanned {} transcripts at {}",
        report.transcripts_scanned,
        report.scanned_at.format("%Y-%m-%d %H:%M UTC")
    );
    println!(
        "Thresholds: warn ≥{}d  alert ≥{}d\n",
        report.warn_days, report.alert_days
    );

    let name_w = report
        .servers
        .iter()
        .map(|s| s.server.len())
        .max()
        .unwrap_or(10)
        .max(10);
    println!(
        "{:<width$}  {:>6}  {:>6}  {:>6}  {:>7}  {:>10}  status",
        "server",
        "7d",
        "14d",
        "30d",
        "total",
        "last (d)",
        width = name_w
    );
    println!("{}", "-".repeat(name_w + 60));
    for s in &report.servers {
        let last = s
            .days_since_last
            .map(|d| format!("{d}"))
            .unwrap_or_else(|| "—".to_string());
        let badge = match s.status {
            Status::Ok => "ok",
            Status::Warn => "WARN",
            Status::Alert => "ALERT",
            Status::Unused => "UNUSED",
        };
        println!(
            "{:<width$}  {:>6}  {:>6}  {:>6}  {:>7}  {:>10}  {}",
            s.server,
            s.calls_7d,
            s.calls_14d,
            s.calls_30d,
            s.calls_total,
            last,
            badge,
            width = name_w
        );
    }
}

pub fn print_idle(idle: &[&ServerReport]) {
    if idle.is_empty() {
        println!("No idle MCP servers. All configured servers used recently.");
        return;
    }
    for s in idle {
        let last = s
            .days_since_last
            .map(|d| format!("{d}d idle"))
            .unwrap_or_else(|| "never called".to_string());
        println!(
            "[{}] {} — {}, {} total calls",
            s.status.label().to_uppercase(),
            s.server,
            last,
            s.calls_total
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            warn_days: 7,
            alert_days: 14,
            ..Default::default()
        }
    }

    #[test]
    fn never_called_with_zero_total_is_unused() {
        assert_eq!(classify(0, None, &cfg()), Status::Unused);
    }

    #[test]
    fn never_called_with_prior_calls_is_alert() {
        // Counted calls but no parseable timestamp — treat as stale.
        assert_eq!(classify(5, None, &cfg()), Status::Alert);
    }

    #[test]
    fn recent_activity_is_ok() {
        assert_eq!(classify(10, Some(0), &cfg()), Status::Ok);
        assert_eq!(classify(10, Some(6), &cfg()), Status::Ok);
    }

    #[test]
    fn warn_window_starts_at_warn_days() {
        assert_eq!(classify(10, Some(7), &cfg()), Status::Warn);
        assert_eq!(classify(10, Some(13), &cfg()), Status::Warn);
    }

    #[test]
    fn alert_threshold_is_inclusive() {
        assert_eq!(classify(10, Some(14), &cfg()), Status::Alert);
        assert_eq!(classify(10, Some(30), &cfg()), Status::Alert);
        assert_eq!(classify(10, Some(365), &cfg()), Status::Alert);
    }

    #[test]
    fn custom_thresholds_are_honored() {
        let cfg = Config {
            warn_days: 3,
            alert_days: 5,
            ..Default::default()
        };
        assert_eq!(classify(10, Some(2), &cfg), Status::Ok);
        assert_eq!(classify(10, Some(3), &cfg), Status::Warn);
        assert_eq!(classify(10, Some(5), &cfg), Status::Alert);
    }

    #[test]
    fn future_dated_timestamps_clamp_to_ok() {
        // Clock skew: a timestamp in the future yields negative days_idle.
        // Should classify as Ok (just called), not wrap around into Alert.
        assert_eq!(classify(10, Some(-3), &cfg()), Status::Ok);
        assert_eq!(classify(10, Some(-365), &cfg()), Status::Ok);
    }
}
