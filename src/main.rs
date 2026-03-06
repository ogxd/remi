use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

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

fn log_file() -> PathBuf {
    remi_dir().join("commits.log")
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

    let script = "#!/bin/sh\nremi hook post-commit\n";
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

fn record_commit() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .output()?;

    if !output.status.success() {
        return Err("git log failed".into());
    }

    let title = String::from_utf8_lossy(&output.stdout);
    let title = title.trim();
    if title.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(remi_dir())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())?;

    writeln!(file, "{title}")?;
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
            println!(
                "remi: commits will be logged to {}",
                log_file().display()
            );
        }
    }
}
