use anyhow::Result;
use clap::{Parser, Subcommand};

mod analyze;
mod cache;
mod config;
mod hook;
mod scan;

#[derive(Parser)]
#[command(
    name = "mcp-pulse",
    version,
    about = "Audit MCP server usage from Claude Code transcripts"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true, help = "Override config file path")]
    config: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Full usage report — table to stdout")]
    Report {
        #[arg(long, help = "Output JSON instead of table")]
        json: bool,
        #[arg(long, help = "Force a fresh scan, ignoring cache")]
        fresh: bool,
    },

    #[command(about = "Show only idle servers (warn/alert)")]
    Idle {
        #[arg(long, help = "Output JSON")]
        json: bool,
    },

    #[command(about = "SessionStart hook entry — silent, writes cache")]
    Hook,

    #[command(about = "Install the SessionStart hook in ~/.claude/settings.json")]
    Install,

    #[command(about = "Remove the SessionStart hook from ~/.claude/settings.json")]
    Uninstall,

    #[command(about = "Show resolved config")]
    ConfigShow,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::load(cli.config.as_deref())?;

    match cli.command {
        Command::Report { json, fresh } => {
            let report = if fresh {
                let stats = scan::scan_all(&cfg)?;
                let report = analyze::build(stats, &cfg)?;
                cache::write(&cfg, &report)?;
                report
            } else {
                cache::read_or_scan(&cfg)?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                analyze::print_table(&report);
            }
        }
        Command::Idle { json } => {
            let report = cache::read_or_scan(&cfg)?;
            let idle: Vec<_> = report
                .servers
                .iter()
                .filter(|s| s.status != analyze::Status::Ok)
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&idle)?);
            } else {
                analyze::print_idle(&idle);
            }
        }
        Command::Hook => {
            hook::run(&cfg)?;
        }
        Command::Install => {
            hook::install()?;
        }
        Command::Uninstall => {
            hook::uninstall()?;
        }
        Command::ConfigShow => {
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
    }
    Ok(())
}
