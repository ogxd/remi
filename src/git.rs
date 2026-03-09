use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::NaiveDate;

#[derive(Clone)]
pub struct CommitEntry {
    pub short_hash: String,
    pub title: String,
    pub repo: String,
    pub repo_path: PathBuf,
    pub timestamp: i64,
}

pub fn git_output(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("git").args(args).output()?;
    if !out.status.success() {
        return Err(format!("git {} failed", args.join(" ")).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Recursively find git repository roots under `root`.
/// Stops descending into a directory once it is identified as a repo.
pub fn find_git_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut queue = vec![root.to_path_buf()];

    while let Some(dir) = queue.pop() {
        if dir.join(".git").exists() {
            repos.push(dir);
            continue; // don't recurse inside a repo
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with('.') {
                        queue.push(path);
                    }
                }
            }
        }
    }
    repos
}

/// Fetch commits by `author_email` from a single repo, within optional date bounds.
/// Uses RS (\x1e) as record separator and US (\x1f) as field separator.
pub fn get_repo_commits(repo: &Path, author_email: &str, start: Option<&str>, end: Option<&str>) -> Vec<CommitEntry> {
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.to_string_lossy().into_owned());

    let mut args = vec![
        "log".to_string(),
        format!("--author={author_email}"),
        "--format=%x1e%h%x1f%s%x1f%at".to_string(),
    ];
    if let Some(s) = start {
        args.push(format!("--after={s}"));
    }
    if let Some(e) = end {
        // Add one day so the end date is inclusive
        if let Ok(d) = NaiveDate::parse_from_str(e, "%Y-%m-%d") {
            let next = d.succ_opt().unwrap_or(d);
            args.push(format!("--before={}", next.format("%Y-%m-%d")));
        } else {
            args.push(format!("--before={e}"));
        }
    }

    let Ok(out) = Command::new("git").args(&args).current_dir(repo).output() else {
        return vec![];
    };

    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split('\x1e')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|record| {
            let parts: Vec<&str> = record.splitn(3, '\x1f').collect();
            if parts.len() < 3 {
                return None;
            }
            let short_hash = parts[0].trim().to_string();
            let title = parts[1].trim().to_string();
            let timestamp: i64 = parts[2].trim().parse().ok()?;
            Some(CommitEntry {
                short_hash,
                title,
                repo: repo_name.clone(),
                repo_path: repo.to_path_buf(),
                timestamp,
            })
        })
        .collect()
}
