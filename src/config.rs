use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_warn_days")]
    pub warn_days: i64,
    #[serde(default = "default_alert_days")]
    pub alert_days: i64,
    #[serde(default = "default_scan_window_days")]
    pub scan_window_days: i64,
    #[serde(default = "default_transcripts_dir")]
    pub transcripts_dir: PathBuf,
    #[serde(default = "default_cache_path")]
    pub cache_path: PathBuf,
}

fn default_warn_days() -> i64 { 7 }
fn default_alert_days() -> i64 { 14 }
fn default_scan_window_days() -> i64 { 30 }
fn default_transcripts_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude").join("projects")
}
fn default_cache_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude").join("cache").join("mcp-pulse.json")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            warn_days: default_warn_days(),
            alert_days: default_alert_days(),
            scan_window_days: default_scan_window_days(),
            transcripts_dir: default_transcripts_dir(),
            cache_path: default_cache_path(),
        }
    }
}

pub fn load(override_path: Option<&Path>) -> Result<Config> {
    let path = override_path.map(PathBuf::from).unwrap_or_else(default_config_path);
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cfg: Config = toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(cfg)
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("mcp-pulse")
        .join("config.toml")
}
