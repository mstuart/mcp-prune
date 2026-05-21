use crate::config::Config;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

    let partials: Vec<HashMap<String, ServerStats>> = transcripts
        .par_iter()
        .filter_map(|p| scan_file(p, now).ok())
        .collect();

    let mut merged: HashMap<String, ServerStats> = HashMap::new();
    for partial in partials {
        for (k, v) in partial {
            let entry = merged.entry(k.clone()).or_insert_with(|| ServerStats { server: k, ..Default::default() });
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
        transcripts_scanned: transcripts.len(),
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
        let line = match line { Ok(l) => l, Err(_) => continue };
        if !line.contains("mcp__") { continue; }
        let v: Value = match serde_json::from_str(&line) { Ok(x) => x, Err(_) => continue };
        let ts = extract_timestamp(&v);

        // 1. Actual tool_use calls
        if let Some(items) = extract_content_items(&v) {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(name) = item.get("name").and_then(Value::as_str) {
                        if let Some(server) = extract_server(name) {
                            let entry = out.entry(server.clone()).or_insert_with(|| ServerStats { server, ..Default::default() });
                            bump_call(entry, ts, now);
                        }
                    }
                }
            }
        }

        // 2. Server appears in deferred-tools listings or system reminders → mark configured
        if let Some(text) = first_text(&v) {
            if text.contains("mcp__") {
                for token in extract_mcp_tokens(text) {
                    if let Some(server) = extract_server(&token) {
                        let entry = out.entry(server.clone()).or_insert_with(|| ServerStats { server, ..Default::default() });
                        entry.configured = true;
                        entry.first_seen = min_opt(entry.first_seen, ts);
                    }
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
        if age_days < 30 { s.calls_30d += 1; }
        if age_days < 14 { s.calls_14d += 1; }
        if age_days < 7 { s.calls_7d += 1; }
    }
}

fn extract_timestamp(v: &Value) -> Option<DateTime<Utc>> {
    let s = v.get("timestamp")?.as_str()?;
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

fn extract_content_items(v: &Value) -> Option<&Vec<Value>> {
    v.get("message")?.get("content")?.as_array()
}

fn first_text(v: &Value) -> Option<&str> {
    let items = extract_content_items(v)?;
    for item in items {
        if let Some(t) = item.get("text").and_then(Value::as_str) {
            return Some(t);
        }
    }
    None
}

fn extract_server(tool_name: &str) -> Option<String> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let mut parts = rest.splitn(2, "__");
    let server = parts.next()?;
    let tool = parts.next()?;
    if server.is_empty() || tool.is_empty() { return None; }
    Some(server.to_string())
}

fn extract_mcp_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices("mcp__") {
        let tail = &text[i..];
        let end = tail.find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-').unwrap_or(tail.len());
        out.push(tail[..end].to_string());
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
