use crate::config::Config;
use crate::installed::{Installed, ServerSource};
use crate::scan::ScanResult;
use crate::style;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Alert,
    Unused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerReport {
    pub server: String,
    pub status: Status,
    #[serde(default)]
    pub source: ServerSource,
    pub calls_total: u64,
    pub calls_30d: u64,
    pub calls_14d: u64,
    pub calls_7d: u64,
    pub last_call: Option<DateTime<Utc>>,
    pub days_since_last: Option<i64>,
    /// True if this server was observed in at least one transcript. False
    /// means it is configured but has never been loaded — the prime prune
    /// candidate: nonzero configuration cost, zero observed usage.
    #[serde(default = "default_true")]
    pub transcript_seen: bool,
    /// Directory that owns the local/project-scope config entry for this
    /// server. `claude mcp remove` only finds project-scope entries when run
    /// from inside the owning directory; `apply` uses this as CWD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub scanned_at: DateTime<Utc>,
    pub transcripts_scanned: usize,
    pub warn_days: i64,
    pub alert_days: i64,
    pub servers: Vec<ServerReport>,
}

/// A presentable slice of a `Report`. `report.servers` is the filtered list
/// (e.g., excluding stale entries when the user didn't pass `--all`);
/// `stale_hidden` records how many were filtered so the footer can mention
/// them without showing the rows.
#[derive(Debug, Clone, Serialize)]
pub struct ReportView {
    #[serde(flatten)]
    pub report: Report,
    pub stale_hidden: usize,
}

pub fn view(report: &Report, include_stale: bool) -> ReportView {
    let total = report.servers.len();
    let servers: Vec<ServerReport> = report
        .servers
        .iter()
        .filter(|s| include_stale || s.source != ServerSource::Historical)
        .cloned()
        .collect();
    let stale_hidden = total - servers.len();
    let mut filtered = report.clone();
    filtered.servers = servers;
    ReportView {
        report: filtered,
        stale_hidden,
    }
}

pub fn idle_view(report: &Report, include_stale: bool) -> ReportView {
    let total_idle = report
        .servers
        .iter()
        .filter(|s| s.status != Status::Ok)
        .count();
    let servers: Vec<ServerReport> = report
        .servers
        .iter()
        .filter(|s| s.status != Status::Ok)
        .filter(|s| include_stale || s.source != ServerSource::Historical)
        .cloned()
        .collect();
    let stale_hidden = total_idle - servers.len();
    let mut filtered = report.clone();
    filtered.servers = servers;
    ReportView {
        report: filtered,
        stale_hidden,
    }
}

pub fn build(scan: ScanResult, installed: &Installed, cfg: &Config) -> Result<Report> {
    let now = scan.scanned_at;
    let mut servers: Vec<ServerReport> = scan
        .servers
        .into_values()
        .map(|s| {
            let days_since_last = s.last_call.map(|t| (now - t).num_days());
            let status = classify(s.calls_total, days_since_last, cfg);
            let source = installed.classify(&s.server);
            let project_dir = installed.project_dir(&s.server).map(|p| p.to_path_buf());
            ServerReport {
                server: s.server,
                status,
                source,
                calls_total: s.calls_total,
                calls_30d: s.calls_30d,
                calls_14d: s.calls_14d,
                calls_7d: s.calls_7d,
                last_call: s.last_call,
                days_since_last,
                transcript_seen: true,
                project_dir,
            }
        })
        .collect();

    // Synthesize entries for configured servers that never appeared in any
    // transcript. Prime prune candidates: configured cost, zero observed
    // usage. Plugin-provided server names are not enumerable from config, so
    // they are excluded from this synthesis.
    let seen: std::collections::HashSet<&str> =
        servers.iter().map(|s| s.server.as_str()).collect();
    let mut synthetic: Vec<ServerReport> = installed
        .all_named()
        .filter(|name| !seen.contains(name.as_str()))
        .map(|name| ServerReport {
            server: name.clone(),
            status: Status::Unused,
            source: installed.classify(name),
            calls_total: 0,
            calls_30d: 0,
            calls_14d: 0,
            calls_7d: 0,
            last_call: None,
            days_since_last: None,
            transcript_seen: false,
            project_dir: installed.project_dir(name).map(|p| p.to_path_buf()),
        })
        .collect();
    servers.append(&mut synthetic);

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

fn glyph(status: Status) -> String {
    match status {
        Status::Ok => style::green("●"),
        Status::Warn => style::amber("◐"),
        Status::Alert => style::red("○"),
        Status::Unused => style::violet("⌀"),
    }
}

fn section_title(status: Status, count: usize, warn_days: i64, alert_days: i64) -> String {
    let label = match status {
        Status::Ok => format!(
            "active ({} server{})",
            count,
            if count == 1 { "" } else { "s" }
        ),
        Status::Warn => format!("idle ≥{}d ({})", warn_days, count),
        Status::Alert => format!("idle ≥{}d ({})", alert_days, count),
        Status::Unused => format!("never called ({})", count),
    };
    format!("{}  {}", glyph(status), style::bold(&label))
}

pub fn print_header(report: &Report) {
    let meta = format!(
        "{} transcripts · {}",
        report.transcripts_scanned,
        report.scanned_at.format("%Y-%m-%d %H:%M UTC"),
    );
    println!("{}  {}", style::bold("mcp-prune"), style::dim(&meta),);
    println!(
        "{}",
        style::dim(&format!(
            "warn ≥{}d  ·  alert ≥{}d",
            report.warn_days, report.alert_days
        ))
    );
    println!();
}

pub fn print_table(view: &ReportView) {
    let report = &view.report;
    print_header(report);

    let groups = [Status::Ok, Status::Warn, Status::Alert, Status::Unused];
    for (i, status) in groups.iter().enumerate() {
        let rows: Vec<&ServerReport> = report
            .servers
            .iter()
            .filter(|s| s.status == *status)
            .collect();
        if rows.is_empty() {
            continue;
        }
        if i != 0 {
            println!();
        }
        println!(
            "  {}",
            section_title(*status, rows.len(), report.warn_days, report.alert_days)
        );
        print_rows(&rows);
    }
    println!();
    print_footer(view);
}

fn print_rows(rows: &[&ServerReport]) {
    let name_w = rows
        .iter()
        .map(|s| s.server.len())
        .max()
        .unwrap_or(10)
        .max(20);
    for s in rows {
        let historical = s.source == ServerSource::Historical;
        let last = s
            .days_since_last
            .map(|d| format!("{}d", d.max(0)))
            .unwrap_or_else(|| "—".to_string());
        let last_painted = if historical {
            style::dim(&last)
        } else {
            match s.status {
                Status::Ok => style::dim(&last),
                Status::Warn => style::amber(&last),
                Status::Alert => style::red(&last),
                Status::Unused => style::dim(&last),
            }
        };
        let total_text = if s.calls_total == 0 {
            "0 calls".to_string()
        } else {
            format!("{} calls", s.calls_total)
        };
        let total = if historical || s.calls_total == 0 {
            style::dim(&total_text)
        } else {
            total_text
        };
        let recent = if historical {
            style::dim("—")
        } else if s.calls_7d > 0 {
            format!("{} in 7d", s.calls_7d)
        } else if s.calls_30d > 0 {
            style::dim(&format!("{} in 30d", s.calls_30d))
        } else {
            style::dim("—")
        };
        let name = if historical {
            style::dim(&s.server)
        } else {
            s.server.clone()
        };
        let tag = source_tag(s);
        let pad = name_w.saturating_sub(s.server.len());
        println!(
            "     {}{}  {:>5}   {:<10}  {}{}",
            name,
            " ".repeat(pad),
            last_painted,
            total,
            recent,
            tag,
        );
    }
}

fn source_tag(s: &ServerReport) -> String {
    let mut tag = match s.source {
        ServerSource::UserConfig => String::new(),
        ServerSource::ProjectConfig => format!("  {}", style::dim("[project]")),
        ServerSource::Plugin => format!("  {}", style::violet("[plugin]")),
        ServerSource::Historical => format!("  {}", style::dim("[stale]")),
    };
    if !s.transcript_seen {
        tag.push_str(&format!("  {}", style::amber("never loaded")));
    }
    tag
}

fn print_footer(view: &ReportView) {
    let actionable_idle = view
        .report
        .servers
        .iter()
        .filter(|s| s.status != Status::Ok && s.source != ServerSource::Historical)
        .count();
    if actionable_idle == 0 {
        println!("  {}", style::dim("→ no idle servers · nothing to prune"));
    } else {
        let msg = format!(
            "→ {} server{} idle · run `mcp-prune apply` to review and remove",
            actionable_idle,
            if actionable_idle == 1 { "" } else { "s" }
        );
        println!("  {}", style::cyan(&msg));
    }
    if view.stale_hidden > 0 {
        println!(
            "  {}",
            style::dim(&format!(
                "  ({} stale entr{} hidden — pass --all to show)",
                view.stale_hidden,
                if view.stale_hidden == 1 { "y" } else { "ies" }
            ))
        );
    }
}

pub fn print_idle_view(view: &ReportView) {
    let idle: Vec<&ServerReport> = view.report.servers.iter().collect();
    if idle.is_empty() && view.stale_hidden == 0 {
        println!();
        println!(
            "  {}  {}",
            style::green("●"),
            style::bold("no idle MCP servers")
        );
        println!("  {}", style::dim("all configured servers used recently"));
        println!();
        return;
    }

    println!();
    let groups = [Status::Warn, Status::Alert, Status::Unused];
    let mut first = true;
    for status in groups {
        let rows: Vec<&ServerReport> = idle
            .iter()
            .copied()
            .filter(|s| s.status == status)
            .collect();
        if rows.is_empty() {
            continue;
        }
        if !first {
            println!();
        }
        first = false;
        let title = match status {
            Status::Warn => format!("warning ({})", rows.len()),
            Status::Alert => format!("alert ({})", rows.len()),
            Status::Unused => format!("never called ({})", rows.len()),
            _ => String::new(),
        };
        println!("  {}  {}", glyph(status), style::bold(&title));
        print_rows(&rows);
    }
    println!();
    let actionable = idle
        .iter()
        .filter(|s| s.source != ServerSource::Historical)
        .count();
    if actionable == 0 {
        println!("  {}", style::dim("→ no actionable idle servers"));
    } else {
        let msg = format!(
            "→ {} idle · run `mcp-prune apply` to review and remove",
            actionable
        );
        println!("  {}", style::cyan(&msg));
    }
    if view.stale_hidden > 0 {
        println!(
            "  {}",
            style::dim(&format!(
                "  ({} stale entr{} hidden — pass --all to show)",
                view.stale_hidden,
                if view.stale_hidden == 1 { "y" } else { "ies" }
            ))
        );
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{ScanResult, ServerStats};
    use chrono::Utc;
    use std::collections::HashMap;

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

    fn empty_scan() -> ScanResult {
        ScanResult {
            servers: HashMap::new(),
            transcripts_scanned: 0,
            scanned_at: Utc::now(),
        }
    }

    fn installed_with(user: &[&str], project: &[&str]) -> Installed {
        let mut i = Installed::default();
        for s in user {
            i.user.insert((*s).into());
        }
        for s in project {
            i.project
                .insert((*s).into(), PathBuf::from("/tmp/test-project"));
        }
        i
    }

    #[test]
    fn build_synthesizes_never_loaded_for_configured_servers_absent_from_scan() {
        let installed = installed_with(&["vexp"], &["figma-remote-mcp", "statsig"]);
        let report = build(empty_scan(), &installed, &cfg()).unwrap();

        // All three configured servers should appear as synthesized entries.
        let names: Vec<&str> = report.servers.iter().map(|s| s.server.as_str()).collect();
        assert!(names.contains(&"vexp"));
        assert!(names.contains(&"figma-remote-mcp"));
        assert!(names.contains(&"statsig"));

        for s in &report.servers {
            assert!(!s.transcript_seen, "synthesized entries are not transcript-seen");
            assert_eq!(s.status, Status::Unused);
            assert_eq!(s.calls_total, 0);
            assert!(s.last_call.is_none());
        }
    }

    #[test]
    fn build_does_not_synthesize_when_server_already_in_scan() {
        let mut scan = empty_scan();
        scan.servers.insert(
            "vexp".into(),
            ServerStats {
                server: "vexp".into(),
                calls_total: 5,
                ..Default::default()
            },
        );
        let installed = installed_with(&["vexp"], &[]);
        let report = build(scan, &installed, &cfg()).unwrap();

        assert_eq!(report.servers.len(), 1);
        let vexp = &report.servers[0];
        assert_eq!(vexp.server, "vexp");
        assert!(vexp.transcript_seen, "scan-derived entries are transcript-seen");
        assert_eq!(vexp.calls_total, 5);
    }

    #[test]
    fn build_tags_synthesized_entries_with_correct_source() {
        let installed = installed_with(&["user-only"], &["project-only"]);
        let report = build(empty_scan(), &installed, &cfg()).unwrap();

        let user = report.servers.iter().find(|s| s.server == "user-only").unwrap();
        assert_eq!(user.source, ServerSource::UserConfig);

        let proj = report.servers.iter().find(|s| s.server == "project-only").unwrap();
        assert_eq!(proj.source, ServerSource::ProjectConfig);
    }
}
