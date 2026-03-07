use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
    process::Command,
};

use chrono::Local;
use log::{error, info, warn};

use crate::config::load_config;
use crate::git::git_output;
use crate::journal::write_entry;
use crate::llm::summarize_diff;
use crate::paths::{daily_log_file, hook_script_path, hooks_dir};
use crate::recap::maybe_generate_recaps;

fn is_hook_installed() -> bool {
    hook_script_path().exists()
}

pub fn install_hook() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = hooks_dir();
    fs::create_dir_all(&hooks)?;

    let remi_bin = std::env::current_exe()?;
    let remi_bin = remi_bin.to_str().ok_or("executable path is not valid UTF-8")?;
    // Source common shell init files so env vars (API keys etc.) are available.
    let script = format!(
        "#!/bin/sh\n\
         [ -f \"$HOME/.zshenv\" ] && . \"$HOME/.zshenv\"\n\
         [ -f \"$HOME/.profile\" ] && . \"$HOME/.profile\"\n\
         {remi_bin} hook post-commit &\n"
    );

    let hook_path = hook_script_path();
    fs::write(&hook_path, &script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }

    let output = Command::new("git").args(["config", "--global", "core.hooksPath"]).output()?;
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

pub fn ensure_hook() {
    if !is_hook_installed() {
        if let Err(e) = install_hook() {
            error!("failed to install git hook: {e}");
        }
    }
}

pub async fn record_commit() -> Result<(), Box<dyn std::error::Error>> {
    let title = git_output(&["log", "-1", "--pretty=%s"])?;
    if title.is_empty() {
        return Ok(());
    }

    let hash = git_output(&["log", "-1", "--pretty=%h"])?;
    let body = git_output(&["log", "-1", "--pretty=%b"]).unwrap_or_default();

    let repo_root = git_output(&["rev-parse", "--show-toplevel"])?;
    let repo = PathBuf::from(&repo_root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(repo_root);

    let time = Local::now().format("%H:%M:%S").to_string();

    let description = if !body.is_empty() {
        Some(body)
    } else {
        match load_config().model {
            Some(model) => {
                let diff = git_output(&["show", "HEAD"]).unwrap_or_default();
                summarize_diff(&diff, &model).await
            }
            None => {
                info!("no model configured, skipping LLM description");
                None
            }
        }
    };

    let log = daily_log_file();
    fs::create_dir_all(log.parent().unwrap())?;

    let mut file = OpenOptions::new().create(true).append(true).open(&log)?;
    write_entry(&mut file, &hash, &title, description.as_deref(), &repo, &time)?;

    info!("recorded commit [{hash}] in {}", log.display());

    if let Some(model) = load_config().model {
        maybe_generate_recaps(&model).await;
    }

    Ok(())
}
