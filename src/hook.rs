use crate::cache;
use crate::config::Config;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// SessionStart hook entry. Spawns a background scan, returns silent OK immediately.
pub fn run(cfg: &Config) -> Result<()> {
    // Slurp stdin in case the harness sends a payload; we don't use it but reading prevents SIGPIPE.
    let mut _buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut _buf);

    // Fire-and-forget background refresh if cache is stale or missing.
    let cache_path = cfg.cache_path.clone();
    let should_refresh = match cache::read(cfg) {
        Ok(Some(_)) => {
            let meta = std::fs::metadata(&cache_path);
            meta.map(|m| {
                m.modified()
                    .map(|t| t.elapsed().map(|e| e.as_secs() > 60 * 60 * 12).unwrap_or(true))
                    .unwrap_or(true)
            })
            .unwrap_or(true)
        }
        _ => true,
    };

    if should_refresh {
        if let Ok(exe) = std::env::current_exe() {
            // Detached background process; ignore stdio.
            let _ = Command::new(exe)
                .arg("report")
                .arg("--fresh")
                .arg("--json")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    // Respond to the hook channel with a quiet OK.
    println!("{}", json!({"continue": true, "suppressOutput": true}));
    Ok(())
}

/// Installer: append a SessionStart hook entry to ~/.claude/settings.json.
pub fn install() -> Result<()> {
    let settings_path = dirs::home_dir()
        .context("no home dir")?
        .join(".claude")
        .join("settings.json");
    let raw = std::fs::read_to_string(&settings_path)
        .with_context(|| format!("read {}", settings_path.display()))?;
    let mut settings: Value = serde_json::from_str(&raw).context("parse settings.json")?;

    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();

    let hooks = settings
        .as_object_mut()
        .context("settings.json root not object")?
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    let hooks_obj = hooks.as_object_mut().context("hooks not object")?;

    let session_start = hooks_obj
        .entry("SessionStart".to_string())
        .or_insert_with(|| json!([]));
    let arr = session_start.as_array_mut().context("SessionStart not array")?;

    let cmd = format!("{} hook", exe_str);
    let already_present = arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(Value::as_array)
            .map(|hs| hs.iter().any(|h| h.get("command").and_then(Value::as_str) == Some(cmd.as_str())))
            .unwrap_or(false)
    });

    if already_present {
        println!("mcp-pulse SessionStart hook already installed.");
        return Ok(());
    }

    arr.push(json!({
        "hooks": [{
            "type": "command",
            "command": cmd
        }]
    }));

    backup(&settings_path)?;
    let new_raw = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&settings_path, new_raw)?;
    println!("Installed mcp-pulse SessionStart hook in {}", settings_path.display());
    Ok(())
}

fn backup(path: &PathBuf) -> Result<()> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let backup_path = path.with_extension(format!("json.bak.{stamp}"));
    std::fs::copy(path, &backup_path)?;
    Ok(())
}
