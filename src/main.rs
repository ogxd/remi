use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

use chrono::Local;

use clap::{Parser, Subcommand};

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

    // Use the absolute path of the current executable so the hook works
    // regardless of whether remi is on the shell PATH during git invocation.
    let remi_bin = std::env::current_exe()?;
    let remi_bin = remi_bin.to_str().ok_or("executable path is not valid UTF-8")?;
    let script = format!("#!/bin/sh\n{remi_bin} hook post-commit\n");

    let hook_path = hook_script_path();
    fs::write(&hook_path, script)?;

    // Make the hook executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }

    // Warn if core.hooksPath is already set to something else
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

fn git_output(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("git").args(args).output()?;
    if !out.status.success() {
        return Err(format!("git {} failed", args.join(" ")).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn record_commit() -> Result<(), Box<dyn std::error::Error>> {
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

    let log = daily_log_file();
    fs::create_dir_all(log.parent().unwrap())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;

    writeln!(file, "- [{time}] [{repo}] [{hash}] {title}")?;
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook {
            hook_command: HookCommands::PostCommit,
        }) => {
            // Called by the git hook — skip ensure_hook() to avoid recursion
            if let Err(e) = record_commit() {
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
