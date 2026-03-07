use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

use chrono::Local;
use clap::{Parser, Subcommand};
use serde::Deserialize;

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
    let script = format!("#!/bin/sh\n{remi_bin} hook post-commit\n");

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
        eprintln!(
            "warning: git core.hooksPath is already set to '{}'. Overwriting with '{}'.",
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

    eprintln!("remi: global git hook installed at {}", hook_path.display());
    Ok(())
}

fn ensure_hook() {
    if !is_hook_installed() {
        if let Err(e) = install_hook() {
            eprintln!("remi: failed to install git hook: {e}");
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
    let response = client.exec_chat(model, req, None).await.ok()?;
    response.content_text_into_string()
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook {
            hook_command: HookCommands::PostCommit,
        }) => {
            if let Err(e) = record_commit().await {
                eprintln!("remi: failed to record commit: {e}");
                std::process::exit(1);
            }
        }
        None => {
            ensure_hook();
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
        None => None,
    };

    let log = daily_log_file();
    fs::create_dir_all(log.parent().unwrap())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;

    writeln!(file, "- [{time}] [{repo}] [{hash}] {title}")?;
    if let Some(summary) = summary {
        writeln!(file, "  - {summary}")?;
    }

    Ok(())
}
