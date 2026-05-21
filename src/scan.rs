use crate::config::Config;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Placeholder names commonly used in MCP documentation/examples. Matched
/// case-insensitively. Filtering these out prevents transcripts that quote
/// docs (e.g., "Pattern: mcp__servername__toolname") from producing fake
/// "configured" entries.
const PLACEHOLDER_NAMES: &[&str] = &[
    "servername",
    "toolname",
    "server",
    "tool",
    "name",
    "example",
    "foo",
    "bar",
    "myserver",
    "my_server",
    "your_server",
    "yourserver",
];

#[derive(Debug, Default, Clone)]
pub struct ServerStats {
    pub server: String,
    pub calls_total: u64,
    pub calls_7d: u64,
    pub calls_14d: u64,
    pub calls_30d: u64,
    pub last_call: Option<DateTime<Utc>>,
    pub first_seen: Option<DateTime<Utc>>,
    pub configured: bool,
}

pub struct ScanResult {
    pub servers: HashMap<String, ServerStats>,
    pub transcripts_scanned: usize,
    pub scanned_at: DateTime<Utc>,
}

pub fn scan_all(cfg: &Config) -> Result<ScanResult> {
    let transcripts = find_transcripts(&cfg.transcripts_dir);
    let now = Utc::now();

    let results: Vec<(PathBuf, Result<HashMap<String, ServerStats>>)> = transcripts
        .par_iter()
        .map(|p| (p.clone(), scan_file(p, now)))
        .collect();

    let mut partials = Vec::with_capacity(results.len());
    let mut failures: Vec<(PathBuf, anyhow::Error)> = Vec::new();
    for (path, res) in results {
        match res {
            Ok(p) => partials.push(p),
            Err(e) => failures.push((path, e)),
        }
    }

    if !failures.is_empty() {
        eprintln!(
            "warning: {} of {} transcript files could not be scanned:",
            failures.len(),
            transcripts.len()
        );
        for (path, err) in failures.iter().take(5) {
            eprintln!("  {}: {err}", path.display());
        }
        if failures.len() > 5 {
            eprintln!("  … and {} more", failures.len() - 5);
        }
    }

    let mut merged: HashMap<String, ServerStats> = HashMap::new();
    for partial in partials {
        for (k, v) in partial {
            let entry = merged.entry(k.clone()).or_insert_with(|| ServerStats {
                server: k,
                ..Default::default()
            });
            entry.calls_total += v.calls_total;
            entry.calls_7d += v.calls_7d;
            entry.calls_14d += v.calls_14d;
            entry.calls_30d += v.calls_30d;
            entry.configured |= v.configured;
            entry.last_call = max_opt(entry.last_call, v.last_call);
            entry.first_seen = min_opt(entry.first_seen, v.first_seen);
        }
    }

    Ok(ScanResult {
        servers: merged,
        transcripts_scanned: transcripts.len() - failures.len(),
        scanned_at: now,
    })
}

fn find_transcripts(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jsonl"))
        .map(|e| e.into_path())
        .collect()
}

fn scan_file(path: &Path, now: DateTime<Utc>) -> Result<HashMap<String, ServerStats>> {
    let f = File::open(path)?;
    let reader = BufReader::new(f);
    let mut out: HashMap<String, ServerStats> = HashMap::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            // I/O error mid-stream means the remainder of this file is
            // untrustworthy; bail rather than silently producing partial
            // counts.
            Err(e) => return Err(e).context("read line"),
        };
        if !line.contains("mcp__") {
            continue;
        }
        // JSON parse errors are expected — Claude Code transcripts may include
        // non-JSON debug lines. Skip and continue.
        let v: Value = match serde_json::from_str(&line) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let ts = extract_timestamp(&v);

        // 1. Actual tool_use calls — authoritative.
        if let Some(items) = extract_content_items(&v) {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(name) = item.get("name").and_then(Value::as_str) {
                        if let Some(server) = extract_server(name) {
                            let entry = out.entry(server.clone()).or_insert_with(|| ServerStats {
                                server,
                                ..Default::default()
                            });
                            bump_call(entry, ts, now);
                        }
                    }
                }
            }
        }

        // 2. Server registered via Claude Code's tool attachment manifest.
        // This is the authoritative source — it's the actual list of MCP tool
        // names made available to the model that turn, not free text.
        for token in extract_attachment_tokens(&v) {
            if let Some(server) = extract_server(&token) {
                let entry = out.entry(server.clone()).or_insert_with(|| ServerStats {
                    server,
                    ..Default::default()
                });
                entry.configured = true;
                entry.first_seen = min_opt(entry.first_seen, ts);
            }
        }

        // 3. Fallback: server appears in message body as a line-prefix token
        // (covers deferred-tool system reminders that aren't in the attachment
        // manifest). Line-start requirement avoids matching prose like
        // "the pattern `mcp__servername__toolname` requires both segments".
        for text in all_text(&v) {
            if !text.contains("mcp__") {
                continue;
            }
            for token in extract_mcp_tokens(text) {
                if let Some(server) = extract_server(&token) {
                    let entry = out.entry(server.clone()).or_insert_with(|| ServerStats {
                        server,
                        ..Default::default()
                    });
                    entry.configured = true;
                    entry.first_seen = min_opt(entry.first_seen, ts);
                }
            }
        }
    }
    Ok(out)
}

fn bump_call(s: &mut ServerStats, ts: Option<DateTime<Utc>>, now: DateTime<Utc>) {
    s.calls_total += 1;
    s.configured = true;
    if let Some(t) = ts {
        s.last_call = max_opt(s.last_call, Some(t));
        s.first_seen = min_opt(s.first_seen, Some(t));
        let age_days = (now - t).num_days();
        if age_days < 30 {
            s.calls_30d += 1;
        }
        if age_days < 14 {
            s.calls_14d += 1;
        }
        if age_days < 7 {
            s.calls_7d += 1;
        }
    }
}

fn extract_timestamp(v: &Value) -> Option<DateTime<Utc>> {
    let s = v.get("timestamp")?.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn extract_content_items(v: &Value) -> Option<&Vec<Value>> {
    v.get("message")?.get("content")?.as_array()
}

fn all_text(v: &Value) -> Vec<&str> {
    let Some(items) = extract_content_items(v) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect()
}

/// Pulls MCP tool names from Claude Code's `attachment.addedNames` /
/// `attachment.addedLines` arrays — these record the actual deferred tool
/// schemas attached on a given turn, so any name found here is by definition
/// a configured tool (not free-text noise).
fn extract_attachment_tokens(v: &Value) -> Vec<String> {
    let Some(att) = v.get("attachment") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in ["addedNames", "addedLines"] {
        if let Some(arr) = att.get(key).and_then(Value::as_array) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if s.starts_with("mcp__") {
                        out.push(s.to_string());
                    }
                }
            }
        }
    }
    out
}

fn extract_server(tool_name: &str) -> Option<String> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    if PLACEHOLDER_NAMES
        .iter()
        .any(|p| p.eq_ignore_ascii_case(server))
    {
        return None;
    }
    Some(server.to_string())
}

fn extract_mcp_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("mcp__") {
            continue;
        }
        let end = trimmed
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .unwrap_or(trimmed.len());
        out.push(trimmed[..end].to_string());
    }
    out
}

fn max_opt<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(std::cmp::max(x, y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

fn min_opt<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(std::cmp::min(x, y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_server_returns_server_segment() {
        assert_eq!(
            extract_server("mcp__github__create_issue"),
            Some("github".to_string())
        );
        assert_eq!(
            extract_server("mcp__gsd-workflow__gsd_cancel"),
            Some("gsd-workflow".to_string())
        );
    }

    #[test]
    fn extract_server_rejects_malformed_tool_names() {
        assert_eq!(extract_server("mcp__"), None);
        assert_eq!(extract_server("mcp__github"), None);
        assert_eq!(extract_server("not_mcp"), None);
        assert_eq!(extract_server(""), None);
        assert_eq!(extract_server("mcp____tool"), None);
    }

    #[test]
    fn extract_server_filters_documentation_placeholders() {
        assert_eq!(extract_server("mcp__SERVERNAME__TOOLNAME"), None);
        assert_eq!(extract_server("mcp__server__tool"), None);
        assert_eq!(extract_server("mcp__servername__toolname"), None);
        assert_eq!(extract_server("mcp__example__do_thing"), None);
        assert_eq!(extract_server("mcp__myserver__do_thing"), None);
    }

    #[test]
    fn extract_tokens_only_matches_line_prefix() {
        let text = "mcp__github__create_issue\n  mcp__gsd__cancel\nuse mcp__embedded__tool here";
        let tokens = extract_mcp_tokens(text);
        assert_eq!(
            tokens,
            vec![
                "mcp__github__create_issue".to_string(),
                "mcp__gsd__cancel".to_string(),
            ]
        );
    }

    #[test]
    fn extract_tokens_ignores_inline_documentation_mentions() {
        let text = "Pattern: mcp__servername__toolname requires both segments after the prefix.";
        assert!(extract_mcp_tokens(text).is_empty());
    }

    #[test]
    fn extract_tokens_handles_indented_lines() {
        let text = "intro\n    mcp__github__list_repos\n";
        assert_eq!(
            extract_mcp_tokens(text),
            vec!["mcp__github__list_repos".to_string()]
        );
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn scan_file_counts_tool_use_calls() {
        // `now` chosen to put 2026-05-20 events inside the 7d window and
        // 2026-04-15 outside the 30d window.
        let now = DateTime::parse_from_rfc3339("2026-05-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = scan_file(&fixture_path("sample.jsonl"), now).unwrap();

        let github = result.get("github").expect("github should be present");
        assert_eq!(github.calls_total, 2, "two tool_use events for github");
        assert_eq!(github.calls_7d, 2);
        assert_eq!(github.calls_14d, 2);
        assert_eq!(github.calls_30d, 2);
        assert!(github.configured);

        let gsd = result.get("gsd-workflow").expect("gsd-workflow present");
        assert_eq!(gsd.calls_total, 1);
        assert_eq!(gsd.calls_7d, 0, "2026-04-15 is outside 7d from 2026-05-22");
        assert_eq!(gsd.calls_14d, 0);
        assert_eq!(gsd.calls_30d, 0, "37 days idle is outside 30d window");
    }

    #[test]
    fn scan_file_marks_attachment_servers_configured_without_calls() {
        let now = DateTime::parse_from_rfc3339("2026-05-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = scan_file(&fixture_path("sample.jsonl"), now).unwrap();

        // `slack` only appears in attachment.addedNames, never as a tool_use.
        let slack = result.get("slack").expect("slack should be configured");
        assert!(slack.configured);
        assert_eq!(slack.calls_total, 0, "addedNames is not a call");
    }

    #[test]
    fn scan_file_ignores_placeholder_text_and_invalid_json() {
        let now = DateTime::parse_from_rfc3339("2026-05-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = scan_file(&fixture_path("sample.jsonl"), now).unwrap();

        // "mcp__servername__toolname" in prose must not produce a "servername" entry.
        assert!(
            !result.contains_key("servername"),
            "placeholder names must not register as servers"
        );
        // Non-JSON line ("not valid json at all") must not crash the scan.
        // The fixture contains a valid line after it; if we got here without
        // panicking and have a non-empty result, parsing recovered.
        assert!(!result.is_empty());
    }

    #[test]
    fn scan_file_picks_up_line_prefix_fallback() {
        let now = DateTime::parse_from_rfc3339("2026-05-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = scan_file(&fixture_path("sample.jsonl"), now).unwrap();

        // The text item "mcp__deferred-server__some_tool" at line-start should
        // register as configured via the text fallback path.
        let deferred = result
            .get("deferred-server")
            .expect("line-prefix fallback should catch this");
        assert!(deferred.configured);
        assert_eq!(deferred.calls_total, 0);
    }
}
