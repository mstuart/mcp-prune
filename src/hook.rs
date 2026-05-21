use crate::cache;
use crate::config::Config;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// SessionStart hook entry. Spawns a background scan, returns silent OK immediately.
pub fn run(cfg: &Config) -> Result<()> {
    // Slurp stdin in case the harness sends a payload. We don't use it, but
    // not reading risks SIGPIPE on the writing side.
    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            eprintln!("mcp-prune hook: unexpected stdin error: {e}");
        }
    }

    if should_refresh(cfg) {
        spawn_background_refresh(cfg);
    }

    // Respond to the hook channel with a quiet OK.
    println!("{}", json!({"continue": true, "suppressOutput": true}));
    Ok(())
}

fn should_refresh(cfg: &Config) -> bool {
    let meta = match std::fs::metadata(&cfg.cache_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Err(e) => {
            eprintln!("mcp-prune hook: cache stat failed: {e}");
            return true;
        }
    };
    let mtime = match meta.modified() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("mcp-prune hook: cache mtime unavailable: {e}");
            return true;
        }
    };
    match mtime.elapsed() {
        Ok(age) => age.as_secs() > cache::CACHE_TTL_SECS,
        Err(_) => true, // future mtime — treat as stale and refresh.
    }
}

fn spawn_background_refresh(cfg: &Config) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mcp-prune hook: current_exe failed: {e}");
            return;
        }
    };

    // Redirect child stderr to a log file beside the cache so background-scan
    // failures are observable. Falls back to Stdio::null if the file can't be
    // opened.
    let log_stderr = cfg
        .cache_path
        .parent()
        .map(|d| d.join("mcp-prune.log"))
        .and_then(|p| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .ok()
                .map(Stdio::from)
        })
        .unwrap_or_else(Stdio::null);

    let spawn_result = Command::new(exe)
        .arg("report")
        .arg("--fresh")
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log_stderr)
        .spawn();

    if let Err(e) = spawn_result {
        eprintln!("mcp-prune hook: could not spawn background refresh: {e}");
    }
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
    let arr = session_start
        .as_array_mut()
        .context("SessionStart not array")?;

    let cmd = format!("{} hook", exe_str);
    let already_present = arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(Value::as_array)
            .map(|hs| {
                hs.iter()
                    .any(|h| h.get("command").and_then(Value::as_str) == Some(cmd.as_str()))
            })
            .unwrap_or(false)
    });

    if already_present {
        println!("mcp-prune SessionStart hook already installed.");
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
    cache::atomic_write(&settings_path, new_raw.as_bytes())?;
    println!(
        "Installed mcp-prune SessionStart hook in {}",
        settings_path.display()
    );
    Ok(())
}

/// Installer: remove every SessionStart hook entry whose command ends with
/// " hook" and contains "mcp-prune" — matches install paths even if the binary
/// has moved (e.g., reinstalled into a different prefix since `install`).
pub fn uninstall() -> Result<()> {
    let settings_path = dirs::home_dir()
        .context("no home dir")?
        .join(".claude")
        .join("settings.json");
    let raw = std::fs::read_to_string(&settings_path)
        .with_context(|| format!("read {}", settings_path.display()))?;
    let mut settings: Value = serde_json::from_str(&raw).context("parse settings.json")?;

    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        println!("No hooks block in settings.json — nothing to uninstall.");
        return Ok(());
    };
    let Some(session_start) = hooks.get_mut("SessionStart").and_then(Value::as_array_mut) else {
        println!("No SessionStart hooks — nothing to uninstall.");
        return Ok(());
    };

    let before = session_start.len();
    session_start.retain(|entry| {
        let Some(hs) = entry.get("hooks").and_then(Value::as_array) else {
            return true;
        };
        !hs.iter().any(|h| {
            let Some(cmd) = h.get("command").and_then(Value::as_str) else {
                return false;
            };
            cmd.contains("mcp-prune") && cmd.trim_end().ends_with("hook")
        })
    });

    if session_start.len() == before {
        println!("mcp-prune SessionStart hook not found — nothing to uninstall.");
        return Ok(());
    }

    backup(&settings_path)?;
    let new_raw = serde_json::to_string_pretty(&settings)?;
    cache::atomic_write(&settings_path, new_raw.as_bytes())?;
    println!(
        "Removed mcp-prune SessionStart hook from {}",
        settings_path.display()
    );
    Ok(())
}

fn backup(path: &Path) -> Result<()> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let backup_path = path.with_extension(format!("json.bak.{stamp}"));
    std::fs::copy(path, &backup_path)?;
    Ok(())
}
