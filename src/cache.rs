use crate::analyze::{self, Report};
use crate::config::Config;
use crate::scan;
use anyhow::{Context, Result};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CACHE_TTL_SECS: u64 = 60 * 60 * 24;

pub fn write(cfg: &Config, report: &Report) -> Result<()> {
    if let Some(parent) = cfg.cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(report)?;
    atomic_write(&cfg.cache_path, json.as_bytes())
}

pub fn read(cfg: &Config) -> Result<Option<Report>> {
    let raw = match fs::read_to_string(&cfg.cache_path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("read cache {}", cfg.cache_path.display()))
        }
    };
    let report: Report = serde_json::from_str(&raw)
        .with_context(|| format!("parse cache {}", cfg.cache_path.display()))?;
    Ok(Some(report))
}

pub fn read_or_scan(cfg: &Config) -> Result<Report> {
    if let Some(report) = read(cfg)? {
        if !is_stale(cfg)? {
            return Ok(report);
        }
    }
    let stats = scan::scan_all(cfg)?;
    let report = analyze::build(stats, cfg)?;
    write(cfg, &report)?;
    Ok(report)
}

fn is_stale(cfg: &Config) -> Result<bool> {
    let meta = fs::metadata(&cfg.cache_path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .context("cache mtime predates Unix epoch")?
        .as_secs();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(now.saturating_sub(mtime) > CACHE_TTL_SECS)
}

/// Write `bytes` to `path` atomically: write to a sibling temp file, then
/// rename. Crash-safe — readers see either the old file or the new one, never
/// a half-written intermediate.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}
