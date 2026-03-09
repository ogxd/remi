use std::{
    fs,
    path::PathBuf,
    process::Command,
};

use log::{error, info, warn};

use crate::git::git_output;
use crate::paths::{hook_script_path, hooks_dir};
use crate::pending::write_pending_commit;

fn is_hook_installed() -> bool {
    hook_script_path().exists()
}

pub fn install_hook() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = hooks_dir();
    fs::create_dir_all(&hooks)?;

    let remi_bin = std::env::current_exe()?;
    let remi_bin = remi_bin.to_str().ok_or("executable path is not valid UTF-8")?;
    let script = format!("#!/bin/sh\n{remi_bin} hook post-commit &\n");

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
    if !is_hook_installed() && let Err(e) = install_hook() {
        error!("failed to install git hook: {e}");
    }
}

pub fn record_commit() -> Result<(), Box<dyn std::error::Error>> {
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

    let timestamp: i64 = git_output(&["log", "-1", "--pretty=%at"])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| chrono::Local::now().timestamp());

    let diff = git_output(&["show", "HEAD"]).unwrap_or_default();

    write_pending_commit(&hash, &repo, &title, timestamp, &diff)?;

    info!("wrote pending commit [{hash}] to {}", crate::paths::pending_dir().display());
    Ok(())
}
