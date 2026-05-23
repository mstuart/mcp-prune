use anyhow::Result;
use clap::{Parser, Subcommand};

mod analyze;
mod apply;
mod cache;
mod config;
mod hook;
mod installed;
mod scan;
mod style;

#[derive(Parser)]
#[command(
    name = "mcp-prune",
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
        #[arg(
            long,
            help = "Include stale entries (servers in transcripts but no longer configured)"
        )]
        all: bool,
    },

    #[command(about = "Show only idle servers (warn/alert)")]
    Idle {
        #[arg(long, help = "Output JSON")]
        json: bool,
        #[arg(long, help = "Include stale entries")]
        all: bool,
    },

    #[command(about = "Review idle servers and remove them via `claude mcp remove`")]
    Apply {
        #[arg(long, help = "Print what would be removed without doing it")]
        dry_run: bool,
        #[arg(long = "yes", short = 'y', help = "Skip confirmation prompts")]
        assume_yes: bool,
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
    style::init();
    let cfg = config::load(cli.config.as_deref())?;

    match cli.command {
        Command::Report { json, fresh, all } => {
            let report = if fresh {
                let stats = scan::scan_all(&cfg)?;
                let inst = installed::load();
                let report = analyze::build(stats, &inst, &cfg)?;
                cache::write(&cfg, &report)?;
                report
            } else {
                cache::read_or_scan(&cfg)?
            };
            let view = analyze::view(&report, all);
            if json {
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else {
                analyze::print_table(&view);
            }
        }
        Command::Idle { json, all } => {
            let report = cache::read_or_scan(&cfg)?;
            let view = analyze::idle_view(&report, all);
            if json {
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else {
                analyze::print_idle_view(&view);
            }
        }
        Command::Apply {
            dry_run,
            assume_yes,
        } => {
            let report = cache::read_or_scan(&cfg)?;
            apply::run(
                &cfg,
                &report,
                apply::ApplyOpts {
                    dry_run,
                    assume_yes,
                },
            )?;
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
