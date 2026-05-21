use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_warn_days")]
    pub warn_days: i64,
    #[serde(default = "default_alert_days")]
    pub alert_days: i64,
    #[serde(default = "default_transcripts_dir")]
    pub transcripts_dir: PathBuf,
    #[serde(default = "default_cache_path")]
    pub cache_path: PathBuf,
}

fn default_warn_days() -> i64 {
    7
}
fn default_alert_days() -> i64 {
    14
}
fn default_transcripts_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("projects")
}
fn default_cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("cache")
        .join("mcp-prune.json")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            warn_days: default_warn_days(),
            alert_days: default_alert_days(),
            transcripts_dir: default_transcripts_dir(),
            cache_path: default_cache_path(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.warn_days < 0 {
            bail!("warn_days must be >= 0 (got {})", self.warn_days);
        }
        if self.alert_days < 0 {
            bail!("alert_days must be >= 0 (got {})", self.alert_days);
        }
        if self.alert_days < self.warn_days {
            bail!(
                "alert_days ({}) must be >= warn_days ({}) — otherwise the warn band is empty",
                self.alert_days,
                self.warn_days
            );
        }
        Ok(())
    }
}

pub fn load(override_path: Option<&Path>) -> Result<Config> {
    let path = override_path
        .map(PathBuf::from)
        .unwrap_or_else(default_config_path);
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cfg: Config = toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    cfg.validate()
        .with_context(|| format!("invalid config in {}", path.display()))?;
    Ok(cfg)
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("mcp-prune")
        .join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn negative_thresholds_rejected() {
        let cfg = Config {
            warn_days: -1,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
        let cfg = Config {
            alert_days: -1,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn alert_below_warn_rejected() {
        let cfg = Config {
            warn_days: 14,
            alert_days: 7,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn equal_thresholds_accepted() {
        let cfg = Config {
            warn_days: 7,
            alert_days: 7,
            ..Default::default()
        };
        cfg.validate().unwrap();
    }
}
