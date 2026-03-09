use std::{path::PathBuf, process::Command, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use log::error;

use crate::git::{find_git_repos, get_repo_commits, git_output};
use crate::paths::remi_dir;
use crate::pending::write_pending_commit;
use crate::recap::maybe_generate_recaps;

pub fn run_scan(path: PathBuf, start: Option<String>, end: Option<String>) {
    let spinner = ProgressBar::new_spinner();
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner.set_message("Scanning for git repositories...");
    let repos = find_git_repos(&path);
    spinner.finish_with_message(format!("Found {} repositories", repos.len()));

    if repos.is_empty() {
        eprintln!("No git repositories found under {}", path.display());
        return;
    }

    let author_email = git_output(&["config", "--global", "user.email"]).unwrap_or_default();
    if author_email.is_empty() {
        eprintln!("remi: could not determine git user.email");
        return;
    }
    log::info!("scanning commits by {author_email}");

    let pb = ProgressBar::new(repos.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} repos scanned ({msg})")
            .unwrap()
            .progress_chars("=> "),
    );

    let mut all_commits = Vec::new();
    for repo in &repos {
        let name = repo.file_name().unwrap_or(repo.as_os_str()).to_string_lossy();
        pb.set_message(format!("{name}"));
        let commits = get_repo_commits(repo, &author_email, start.as_deref(), end.as_deref());
        all_commits.extend(commits);
        pb.inc(1);
    }
    pb.finish_with_message(format!("{} commits collected", all_commits.len()));

    if all_commits.is_empty() {
        eprintln!("No commits found.");
        return;
    }

    all_commits.sort_by_key(|c| c.timestamp);

    let total = all_commits.len() as u64;
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} commits processed ({msg})")
            .unwrap()
            .progress_chars("=> "),
    );

    for commit in &all_commits {
        pb.set_message(commit.short_hash.to_string());

        let diff = Command::new("git")
            .args(["show", &commit.short_hash])
            .current_dir(&commit.repo_path)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();

        if let Err(e) = write_pending_commit(&commit.short_hash, &commit.repo, &commit.title, commit.timestamp, &diff) {
            error!("failed to write pending commit {}: {e}", commit.short_hash);
        }

        pb.inc(1);
    }
    pb.finish_with_message("done");

    eprintln!("Wrote {} pending commits to {}", all_commits.len(), remi_dir().display());

    maybe_generate_recaps();
}
