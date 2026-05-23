use crate::analyze::{Report, ServerReport, Status};
use crate::installed::ServerSource;
use crate::style;
use anyhow::{bail, Context, Result};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;

pub struct ApplyOpts {
    pub dry_run: bool,
    pub assume_yes: bool,
}

pub fn run(report: &Report, opts: ApplyOpts) -> Result<()> {
    let idle: Vec<&ServerReport> = report
        .servers
        .iter()
        .filter(|s| s.status != Status::Ok)
        .collect();

    if idle.is_empty() {
        println!();
        println!(
            "  {}  {}",
            style::green("●"),
            style::bold("no idle MCP servers to prune")
        );
        println!();
        return Ok(());
    }

    let actionable = idle
        .iter()
        .filter(|s| !matches!(s.source, ServerSource::Historical))
        .count();
    let stale = idle.len() - actionable;
    println!();
    let mode = if opts.dry_run {
        "dry-run (nothing will be removed)"
    } else if opts.assume_yes {
        "auto-confirm enabled"
    } else {
        "prompting per server"
    };
    let mut header = format!(
        "{} idle server{} · {}",
        actionable,
        if actionable == 1 { "" } else { "s" },
        mode
    );
    if stale > 0 {
        header.push_str(&format!(" · {} stale skipped", stale));
    }
    println!("  {}  {}", style::cyan("apply"), style::dim(&header));
    println!();

    let mut stdin = io::BufReader::new(io::stdin());
    let mut removed = 0;
    let mut skipped = 0;
    let mut plugin_skipped = 0;
    let mut historical_skipped = 0;

    for s in &idle {
        match s.source {
            ServerSource::Plugin => {
                print_plugin_hint(s);
                plugin_skipped += 1;
                continue;
            }
            ServerSource::Historical => {
                // Silently skip — nothing to remove. Reported in summary.
                historical_skipped += 1;
                continue;
            }
            ServerSource::UserConfig | ServerSource::ProjectConfig => {}
        }
        let action = decide(s, &opts, &mut stdin)?;
        match action {
            Decision::Remove => {
                if opts.dry_run {
                    let where_clause = s
                        .project_dir
                        .as_ref()
                        .map(|p| format!(" (in {})", p.display()))
                        .unwrap_or_default();
                    println!(
                        "     {}  would run: claude mcp remove {}{}",
                        style::dim("·"),
                        s.server,
                        where_clause,
                    );
                    removed += 1;
                } else {
                    match run_remove(&s.server, s.project_dir.as_deref()) {
                        Ok(()) => {
                            println!(
                                "     {}  removed {}",
                                style::green("✓"),
                                style::bold(&s.server)
                            );
                            removed += 1;
                        }
                        Err(e) => {
                            println!(
                                "     {}  failed to remove {}: {}",
                                style::red("✗"),
                                s.server,
                                e
                            );
                        }
                    }
                }
            }
            Decision::Skip => {
                skipped += 1;
            }
            Decision::Abort => {
                println!();
                println!("  {}", style::dim("aborted"));
                println!();
                return Ok(());
            }
        }
        println!();
    }

    print_summary(
        removed,
        skipped,
        plugin_skipped,
        historical_skipped,
        opts.dry_run,
    );
    Ok(())
}

enum Decision {
    Remove,
    Skip,
    Abort,
}

fn decide(
    s: &ServerReport,
    opts: &ApplyOpts,
    stdin: &mut io::BufReader<io::Stdin>,
) -> Result<Decision> {
    print_server_card(s);

    if opts.assume_yes || opts.dry_run {
        return Ok(Decision::Remove);
    }

    loop {
        print!("     {} ", style::cyan("remove? [y/N/q]"));
        io::stdout().flush().ok();
        let mut line = String::new();
        let n = stdin.read_line(&mut line).context("read stdin")?;
        if n == 0 {
            // EOF — treat as quit.
            return Ok(Decision::Abort);
        }
        match line.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(Decision::Remove),
            "" | "n" | "no" => return Ok(Decision::Skip),
            "q" | "quit" => return Ok(Decision::Abort),
            other => {
                println!(
                    "     {} unknown response {:?}; expected y, n, or q",
                    style::red("·"),
                    other
                );
            }
        }
    }
}

fn print_server_card(s: &ServerReport) {
    let badge = match s.status {
        Status::Warn => style::amber("warn"),
        Status::Alert => style::red("alert"),
        Status::Unused => style::violet("unused"),
        Status::Ok => style::green("ok"),
    };
    let last = s
        .days_since_last
        .map(|d| format!("{}d idle", d.max(0)))
        .unwrap_or_else(|| "never called".to_string());
    println!(
        "  {}  {}  {}",
        badge,
        style::bold(&s.server),
        style::dim(&last)
    );
    println!(
        "     {}  {} total · {} in last 30d · {} in last 7d",
        style::dim("usage"),
        s.calls_total,
        s.calls_30d,
        s.calls_7d
    );
}

fn print_plugin_hint(s: &ServerReport) {
    let plugin_name = s.server.trim_start_matches("plugin_");
    println!(
        "  {}  {}  {}",
        style::violet("plugin"),
        style::bold(&s.server),
        style::dim("not removed automatically")
    );
    println!(
        "     {}  this is a plugin-defined server; use `claude plugin disable {}`",
        style::dim("hint"),
        first_plugin_segment(plugin_name)
    );
    println!();
}

fn first_plugin_segment(after_prefix: &str) -> &str {
    after_prefix.split('_').next().unwrap_or(after_prefix)
}

fn run_remove(name: &str, project_dir: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("claude");
    cmd.arg("mcp").arg("remove").arg(name);
    // Project-scope entries (`projects[<dir>].mcpServers` in ~/.claude.json,
    // or a project's `.mcp.json`) are only visible to `claude mcp remove` when
    // it runs in that directory. User-scope entries are visible from anywhere.
    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }
    let output = cmd.output().context("spawn `claude mcp remove`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "claude mcp remove exited {}: {}",
            output.status,
            stderr.trim()
        );
    }
    Ok(())
}

fn print_summary(
    removed: usize,
    skipped: usize,
    plugin_skipped: usize,
    historical_skipped: usize,
    dry_run: bool,
) {
    let verb = if dry_run { "would remove" } else { "removed" };
    let mut parts = vec![format!("{} {}", verb, removed)];
    if skipped > 0 {
        parts.push(format!("skipped {}", skipped));
    }
    if plugin_skipped > 0 {
        parts.push(format!("plugins ignored {}", plugin_skipped));
    }
    if historical_skipped > 0 {
        parts.push(format!("stale ignored {}", historical_skipped));
    }
    println!(
        "  {}  {}",
        style::cyan("done"),
        style::dim(&parts.join(" · "))
    );
    println!();
}
