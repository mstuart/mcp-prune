use crate::analyze::{self, Report};
use crate::config::Config;
use crate::scan;
use anyhow::Result;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_TTL_SECS: u64 = 60 * 60 * 24;

pub fn write(cfg: &Config, report: &Report) -> Result<()> {
    if let Some(parent) = cfg.cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(report)?;
    fs::write(&cfg.cache_path, json)?;
    Ok(())
}

pub fn read(cfg: &Config) -> Result<Option<Report>> {
    if !cfg.cache_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&cfg.cache_path)?;
    let report: Report = serde_json::from_str(&raw)?;
    Ok(Some(report))
}

pub fn read_or_scan(cfg: &Config) -> Result<Report> {
    if let Some(report) = read(cfg)? {
        if !is_stale(cfg, &report)? {
            return Ok(report);
        }
    }
    let stats = scan::scan_all(cfg)?;
    let report = analyze::build(stats, cfg)?;
    write(cfg, &report)?;
    Ok(report)
}

fn is_stale(cfg: &Config, _report: &Report) -> Result<bool> {
    let meta = fs::metadata(&cfg.cache_path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(now.saturating_sub(mtime) > CACHE_TTL_SECS)
}
