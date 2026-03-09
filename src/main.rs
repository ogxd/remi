use std::fs::OpenOptions;

use clap::{Parser, Subcommand};
use log::error;
use simplelog::{Config as LogConfig, LevelFilter, WriteLogger};

mod git;
mod hook;
mod journal;
mod paths;
mod pending;
mod recap;
mod scan;

use hook::{ensure_hook, record_commit as hook_record_commit};
use paths::remi_dir;
use recap::{maybe_generate_recaps, run_recap};
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
    /// Install the git hook, optionally scan repos, queue recaps, then output pending items
    Check {
        /// Root path to scan for git repositories (optional, for first-time setup or backfill)
        path: Option<std::path::PathBuf>,
        /// Only include commits/recaps on or after this date (YYYY-MM-DD)
        #[arg(long, value_name = "YYYY-MM-DD")]
        start: Option<String>,
        /// Only include commits/recaps on or before this date (YYYY-MM-DD)
        #[arg(long, value_name = "YYYY-MM-DD")]
        end: Option<String>,
    },
    /// Record a processed item (commit summary or recap completion)
    Record {
        #[command(subcommand)]
        record_command: RecordCommands,
    },
    /// Queue recap.md generation for complete past months and years
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

#[derive(Subcommand)]
enum RecordCommands {
    /// Write a journal entry for a summarized commit and remove its pending file
    Commit {
        /// Short commit hash
        hash: String,
        /// One-sentence summary of the commit
        summary: String,
    },
    /// Remove the pending file for a recap that has been written to disk
    Recap {
        /// Period identifier (e.g. 2025-01 for month, 2025 for year)
        period: String,
    },
}

fn init_logger() {
    let remi = remi_dir();
    if let Ok(()) = std::fs::create_dir_all(&remi)
        && let Ok(file) = OpenOptions::new().create(true).append(true).open(remi.join("remi.log"))
    {
        let _ = WriteLogger::init(LevelFilter::Info, LogConfig::default(), file);
    }
}

fn main() {
    init_logger();
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook {
            hook_command: HookCommands::PostCommit,
        }) => {
            if let Err(e) = hook_record_commit() {
                error!("failed to record commit: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Check { path, start, end }) => {
            ensure_hook();
            if let Some(p) = path {
                run_scan(p, start, end);
            } else {
                maybe_generate_recaps();
            }
            pending::run_check();
        }
        Some(Commands::Record { record_command }) => match record_command {
            RecordCommands::Commit { hash, summary } => {
                if let Err(e) = pending::record_commit(&hash, &summary) {
                    error!("failed to record commit: {e}");
                    eprintln!("remi: {e}");
                    std::process::exit(1);
                }
            }
            RecordCommands::Recap { period } => {
                if let Err(e) = pending::record_recap(&period) {
                    error!("failed to record recap: {e}");
                    eprintln!("remi: {e}");
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Recap { start, end }) => {
            run_recap(start, end);
        }
        Some(Commands::Scan { path, start, end }) => {
            run_scan(path, start, end);
        }
        None => {
            ensure_hook();
            log::info!("commits will be logged under {}", remi_dir().display());
            println!("remi: commits will be logged under {}", remi_dir().display());
        }
    }
}
