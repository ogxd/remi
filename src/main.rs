use std::fs::OpenOptions;

use clap::{Parser, Subcommand};
use log::error;
use simplelog::{Config as LogConfig, LevelFilter, WriteLogger};

mod config;
mod git;
mod hook;
mod journal;
mod llm;
mod paths;
mod recap;
mod scan;

use hook::{ensure_hook, record_commit};
use paths::remi_dir;
use recap::run_recap;
use scan::run_scan;

#[derive(Parser)]
#[command(name = "remi", about = "Remi – your commit journal")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Called internally by the git post-commit hook
    Hook {
        #[command(subcommand)]
        hook_command: HookCommands,
    },
    /// (Re)generate recap.md files for complete past months and years
    Recap {
        /// Only recap periods on or after this date (YYYY-MM-DD)
        #[arg(long, value_name = "YYYY-MM-DD")]
        start: Option<String>,
        /// Only recap periods on or before this date (YYYY-MM-DD)
        #[arg(long, value_name = "YYYY-MM-DD")]
        end: Option<String>,
    },
    /// Scan a directory for git repositories and backfill the journal
    Scan {
        /// Root path to search for git repositories
        path: std::path::PathBuf,
        /// Only include commits on or after this date (YYYY-MM-DD)
        #[arg(long, value_name = "YYYY-MM-DD")]
        start: Option<String>,
        /// Only include commits on or before this date (YYYY-MM-DD)
        #[arg(long, value_name = "YYYY-MM-DD")]
        end: Option<String>,
    },
}

#[derive(Subcommand)]
enum HookCommands {
    /// Records the latest commit title to the log (invoked by post-commit hook)
    PostCommit,
}

fn init_logger() {
    let remi = remi_dir();
    if let Ok(()) = std::fs::create_dir_all(&remi) {
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(remi.join("remi.log")) {
            let _ = WriteLogger::init(LevelFilter::Info, LogConfig::default(), file);
        }
    }
}

#[tokio::main]
async fn main() {
    init_logger();
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook {
            hook_command: HookCommands::PostCommit,
        }) => {
            if let Err(e) = record_commit().await {
                error!("failed to record commit: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Recap { start, end }) => {
            run_recap(start, end).await;
        }
        Some(Commands::Scan { path, start, end }) => {
            run_scan(path, start, end).await;
        }
        None => {
            ensure_hook();
            log::info!("commits will be logged under {}", remi_dir().display());
            println!("remi: commits will be logged under {}", remi_dir().display());
        }
    }
}
