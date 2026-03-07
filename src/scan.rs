use std::{collections::BTreeMap, fs::File, path::PathBuf, process::Command, sync::Arc, time::Duration};

use tokio::sync::Semaphore;

use chrono::{Local, TimeZone};
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use log::error;

use crate::config::load_config;
use crate::git::{find_git_repos, get_repo_commits, git_output};
use crate::journal::write_entry;
use crate::llm::summarize_diff;
use crate::paths::{daily_log_file_for, remi_dir};
use crate::recap::maybe_generate_recaps;

pub async fn run_scan(path: PathBuf, start: Option<String>, end: Option<String>) {
    let spinner = ProgressBar::new_spinner();
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner.set_message("Scanning for git repositories...");
    let repos = find_git_repos(&path);
    spinner.finish_with_message(format!("Found {} repositories", repos.len()));

    if repos.is_empty() {
        println!("No git repositories found under {}", path.display());
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
        println!("No commits found.");
        return;
    }

    all_commits.sort_by_key(|c| c.timestamp);

    let mut by_day: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for commit in all_commits {
        let date = Local
            .timestamp_opt(commit.timestamp, 0)
            .single()
            .map(|dt| dt.date_naive())
            .unwrap_or_else(|| Local::now().date_naive());
        by_day.entry(date).or_default().push(commit);
    }

    let total_commits: u64 = by_day.values().map(|v| v.len() as u64).sum();
    let pb = ProgressBar::new(total_commits);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} commits summarized ({msg})")
            .unwrap()
            .progress_chars("=> "),
    );

    let model: Arc<Option<String>> = Arc::new(load_config().model);
    let num_days = by_day.len();
    let sem = Arc::new(Semaphore::new(8));

    let day_futs = by_day.into_iter().map(|(date, commits)| {
        let pb = pb.clone();
        let model = Arc::clone(&model);
        let sem = Arc::clone(&sem);
        async move {
            let desc_futs = commits.iter().map(|commit| {
                let model = Arc::clone(&model);
                let pb = pb.clone();
                let sem = Arc::clone(&sem);
                let hash = commit.short_hash.clone();
                let body = commit.body.clone();
                let repo_path = commit.repo_path.clone();
                async move {
                    if !body.is_empty() {
                        return Some(body);
                    }
                    let Some(ref m) = *model else {
                        return None;
                    };
                    let _permit = sem.acquire().await.unwrap();
                    pb.set_message(format!("{hash}"));
                    let diff = Command::new("git")
                        .args(["show", &hash])
                        .current_dir(&repo_path)
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default();
                    summarize_diff(&diff, m).await
                }
            });
            let descriptions: Vec<Option<String>> = join_all(desc_futs).await;

            // Write file once all descriptions are ready
            let log_path = daily_log_file_for(&date);
            if let Err(e) = std::fs::create_dir_all(log_path.parent().unwrap()) {
                error!("failed to create dir for {}: {e}", log_path.display());
                pb.inc(commits.len() as u64);
                return;
            }
            match File::create(&log_path) {
                Ok(mut file) => {
                    for (commit, desc) in commits.iter().zip(descriptions) {
                        let dt = Local.timestamp_opt(commit.timestamp, 0).single().unwrap_or_else(|| Local::now());
                        let time_str = dt.format("%H:%M:%S").to_string();
                        if let Err(e) = write_entry(
                            &mut file,
                            &commit.short_hash,
                            &commit.title,
                            desc.as_deref(),
                            &commit.repo,
                            &time_str,
                        ) {
                            error!("failed to write entry {}: {e}", commit.short_hash);
                        }
                        pb.inc(1);
                    }
                    log::info!("wrote {} commits to {}", commits.len(), log_path.display());
                }
                Err(e) => {
                    error!("failed to create {}: {e}", log_path.display());
                    pb.inc(commits.len() as u64);
                }
            }
        }
    });

    join_all(day_futs).await;
    pb.finish_with_message("done");

    println!("Wrote {} days of logs to {}", num_days, remi_dir().display());

    if let Some(model) = load_config().model {
        maybe_generate_recaps(&model).await;
    }
}
