use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

use chrono::Local;
use clap::{Parser, Subcommand};
use log::{error, info, warn};
use serde::Deserialize;
use simplelog::{Config as LogConfig, LevelFilter, WriteLogger};

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
}

#[derive(Subcommand)]
enum HookCommands {
    /// Records the latest commit title to the log (invoked by post-commit hook)
    PostCommit,
}

#[derive(Deserialize, Default)]
struct Config {
    model: Option<String>,
}

fn remi_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".remi")
}

fn hooks_dir() -> PathBuf {
    remi_dir().join("hooks")
}

fn daily_log_file() -> PathBuf {
    let now = Local::now();
    let year = now.format("%Y").to_string();
    let month = now.format("%m").to_string();
    let filename = now.format("%d-%m-%Y.md").to_string();
    remi_dir().join(year).join(month).join(filename)
}

fn hook_script_path() -> PathBuf {
    hooks_dir().join("post-commit")
}

fn is_hook_installed() -> bool {
    hook_script_path().exists()
}

fn install_hook() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = hooks_dir();
    fs::create_dir_all(&hooks)?;

    let remi_bin = std::env::current_exe()?;
    let remi_bin = remi_bin.to_str().ok_or("executable path is not valid UTF-8")?;
    // Source common shell init files so env vars (API keys etc.) are available.
    // .zshenv is loaded by zsh for all shells; .profile covers bash/sh.
    let script = format!(
        "#!/bin/sh\n\
         [ -f \"$HOME/.zshenv\" ] && . \"$HOME/.zshenv\"\n\
         [ -f \"$HOME/.profile\" ] && . \"$HOME/.profile\"\n\
         {remi_bin} hook post-commit\n"
    );

    let hook_path = hook_script_path();
    fs::write(&hook_path, &script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }

    let output = Command::new("git")
        .args(["config", "--global", "core.hooksPath"])
        .output()?;
    let current = String::from_utf8_lossy(&output.stdout);
    let current = current.trim();
    if !current.is_empty() && current != hooks.to_str().unwrap_or("") {
        warn!(
            "git core.hooksPath is already set to '{}'. Overwriting with '{}'.",
            current,
            hooks.display()
        );
    }

    Command::new("git")
        .args([
            "config",
            "--global",
            "core.hooksPath",
            hooks.to_str().expect("hooks path not valid UTF-8"),
        ])
        .status()?;

    info!("global git hook installed at {}", hook_path.display());
    Ok(())
}

fn ensure_hook() {
    if !is_hook_installed() {
        if let Err(e) = install_hook() {
            error!("failed to install git hook: {e}");
        }
    }
}

fn load_config() -> Config {
    let path = remi_dir().join("config.toml");
    let Ok(contents) = fs::read_to_string(&path) else {
        return Config::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

fn git_output(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("git").args(args).output()?;
    if !out.status.success() {
        return Err(format!("git {} failed", args.join(" ")).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn summarize_diff(diff: &str, model: &str) -> Option<String> {
    use genai::chat::{ChatMessage, ChatRequest};
    use genai::Client;

    const MAX_DIFF_CHARS: usize = 8000;
    let diff = if diff.len() > MAX_DIFF_CHARS {
        warn!("diff truncated to {MAX_DIFF_CHARS} chars for LLM summarization");
        &diff[..MAX_DIFF_CHARS]
    } else {
        diff
    };

    let prompt = format!(
        "Summarize the following git diff in one concise sentence, \
         focusing on what changed and why it matters. \
         Reply with only the sentence, no preamble.\n\n{diff}"
    );

    let req = ChatRequest::new(vec![ChatMessage::user(prompt)]);
    let client = Client::default();
    info!("requesting diff summary from model '{model}'");
    match client.exec_chat(model, req, None).await {
        Ok(response) => {
            let summary = response.content_text_into_string();
            if summary.is_none() {
                warn!("LLM returned an empty response");
            }
            summary
        }
        Err(e) => {
            error!("LLM call failed: {e}");
            None
        }
    }
}

fn init_logger() {
    let log_path = remi_dir().join("remi.log");
    if let Ok(()) = fs::create_dir_all(remi_dir()) {
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
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
        None => {
            ensure_hook();
            info!("commits will be logged under {}", remi_dir().display());
            println!("remi: commits will be logged under {}", remi_dir().display());
        }
    }
}

async fn record_commit() -> Result<(), Box<dyn std::error::Error>> {
    let title = git_output(&["log", "-1", "--pretty=%s"])?;
    if title.is_empty() {
        return Ok(());
    }

    let hash = git_output(&["log", "-1", "--pretty=%h"])?;
    let description = git_output(&["log", "-1", "--pretty=%b"]).unwrap_or_default();

    let repo_root = git_output(&["rev-parse", "--show-toplevel"])?;
    let repo = PathBuf::from(&repo_root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(repo_root);

    let time = Local::now().format("%H:%M:%S").to_string();

    let summary = match load_config().model {
        Some(model) => {
            let diff = git_output(&["show", "HEAD"]).unwrap_or_default();
            summarize_diff(&diff, &model).await
        }
        None => {
            info!("no model configured, skipping diff summary");
            None
        }
    };

    let log = daily_log_file();
    fs::create_dir_all(log.parent().unwrap())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;

    writeln!(file, "- [{time}] Commit {hash} on repository \"{repo}\"")?;
    writeln!(file, "  - Message: {title}")?;
    if !description.is_empty() {
        writeln!(file, "  - Description: {description}")?;
    }
    if let Some(summary) = summary {
        let mut lines = summary.lines();
        if let Some(first) = lines.next() {
            writeln!(file, "  - Summary: {first}")?;
            for line in lines {
                if line.trim().is_empty() {
                    writeln!(file)?;
                } else {
                    writeln!(file, "    {line}")?;
                }
            }
        }
    }

    info!("recorded commit [{hash}] in {}", log.display());
    Ok(())
}
