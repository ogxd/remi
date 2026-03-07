use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use chrono::{Local, NaiveDate, TimeZone};
use futures::future::join_all;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info, warn};
use serde::Deserialize;
use simplelog::{Config as LogConfig, LevelFilter, WriteLogger};

// ── CLI ──────────────────────────────────────────────────────────────────────

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
    /// (Re)generate recap.md files for complete past months and years
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
        path: PathBuf,
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

// ── Config ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct Config {
    model: Option<String>,
}

fn load_config() -> Config {
    let path = remi_dir().join("config.toml");
    let Ok(contents) = fs::read_to_string(&path) else {
        return Config::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

// ── Paths ────────────────────────────────────────────────────────────────────

fn remi_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".remi")
}

fn hooks_dir() -> PathBuf {
    remi_dir().join("hooks")
}

fn daily_log_file() -> PathBuf {
    daily_log_file_for(&Local::now().date_naive())
}

fn daily_log_file_for(date: &NaiveDate) -> PathBuf {
    let year = date.format("%Y").to_string();
    let month = date.format("%m").to_string();
    let filename = date.format("%d-%m-%Y.md").to_string();
    remi_dir().join(year).join(month).join(filename)
}

fn hook_script_path() -> PathBuf {
    hooks_dir().join("post-commit")
}

fn is_hook_installed() -> bool {
    hook_script_path().exists()
}

// ── Hook install ─────────────────────────────────────────────────────────────

fn install_hook() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = hooks_dir();
    fs::create_dir_all(&hooks)?;

    let remi_bin = std::env::current_exe()?;
    let remi_bin = remi_bin.to_str().ok_or("executable path is not valid UTF-8")?;
    // Source common shell init files so env vars (API keys etc.) are available.
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

// ── Git helpers ───────────────────────────────────────────────────────────────

fn git_output(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("git").args(args).output()?;
    if !out.status.success() {
        return Err(format!("git {} failed", args.join(" ")).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ── LLM ──────────────────────────────────────────────────────────────────────

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

// ── Log writing ───────────────────────────────────────────────────────────────

/// Writes one commit entry in the standard remi format.
fn write_entry(
    file: &mut impl Write,
    short_hash: &str,
    title: &str,
    description: Option<&str>,
    repo: &str,
    time_str: &str,
) -> std::io::Result<()> {
    writeln!(file, "- [{time_str}] Commit {short_hash} on repository \"{repo}\"")?;
    writeln!(file, "  - Message: {title}")?;
    if let Some(desc) = description {
        if !desc.is_empty() {
            let mut lines = desc.lines();
            if let Some(first) = lines.next() {
                writeln!(file, "  - Description: {first}")?;
                for line in lines {
                    if line.trim().is_empty() {
                        writeln!(file)?;
                    } else {
                        writeln!(file, "    {line}")?;
                    }
                }
            }
        }
    }
    Ok(())
}

// ── post-commit hook handler ──────────────────────────────────────────────────

async fn record_commit() -> Result<(), Box<dyn std::error::Error>> {
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

// ── scan ──────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct CommitEntry {
    short_hash: String,
    title: String,
    body: String,
    repo: String,
    repo_path: PathBuf,
    timestamp: i64,
}

/// Recursively find git repository roots under `root`.
/// Stops descending into a directory once it is identified as a repo.
fn find_git_repos(root: &Path) -> Vec<PathBuf> {
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
fn get_repo_commits(
    repo: &Path,
    author_email: &str,
    start: Option<&str>,
    end: Option<&str>,
) -> Vec<CommitEntry> {
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.to_string_lossy().into_owned());

    let mut args = vec![
        "log".to_string(),
        format!("--author={author_email}"),
        "--format=%x1e%h%x1f%s%x1f%at%x1f%b".to_string(),
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

    let Ok(out) = Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
    else {
        return vec![];
    };

    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split('\x1e')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|record| {
            let parts: Vec<&str> = record.splitn(4, '\x1f').collect();
            if parts.len() < 3 {
                return None;
            }
            let short_hash = parts[0].trim().to_string();
            let title = parts[1].trim().to_string();
            let timestamp: i64 = parts[2].trim().parse().ok()?;
            let body = parts.get(3).map(|s| s.trim().to_string()).unwrap_or_default();
            Some(CommitEntry {
                short_hash,
                title,
                body,
                repo: repo_name.clone(),
                repo_path: repo.to_path_buf(),
                timestamp,
            })
        })
        .collect()
}

async fn run_scan(path: PathBuf, start: Option<String>, end: Option<String>) {
    // ── 1. Discover repos ────────────────────────────────────────────────────
    let spinner = ProgressBar::new_spinner();
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner.set_message("Scanning for git repositories...");
    let repos = find_git_repos(&path);
    spinner.finish_with_message(format!("Found {} repositories", repos.len()));

    if repos.is_empty() {
        println!("No git repositories found under {}", path.display());
        return;
    }

    // ── 2. Get current user email ────────────────────────────────────────────
    let author_email = git_output(&["config", "--global", "user.email"])
        .unwrap_or_default();
    if author_email.is_empty() {
        eprintln!("remi: could not determine git user.email");
        return;
    }
    info!("scanning commits by {author_email}");

    // ── 3. Collect commits from every repo ───────────────────────────────────
    let pb = ProgressBar::new(repos.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} repos  {msg}",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    let mut all_commits: Vec<CommitEntry> = Vec::new();
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

    // ── 4. Sort oldest → newest ──────────────────────────────────────────────
    all_commits.sort_by_key(|c| c.timestamp);

    // ── 5. Group by date ─────────────────────────────────────────────────────
    let mut by_day: BTreeMap<NaiveDate, Vec<CommitEntry>> = BTreeMap::new();
    for commit in all_commits {
        let date = Local
            .timestamp_opt(commit.timestamp, 0)
            .single()
            .map(|dt| dt.date_naive())
            .unwrap_or_else(|| Local::now().date_naive());
        by_day.entry(date).or_default().push(commit);
    }

    // ── 6. Write log files (days in parallel, LLM calls parallel within each day) ──
    let total_commits: u64 = by_day.values().map(|v| v.len() as u64).sum();
    let pb = ProgressBar::new(total_commits);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} commits  {msg}",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    let model: Arc<Option<String>> = Arc::new(load_config().model);
    let num_days = by_day.len();

    let day_futs = by_day.into_iter().map(|(date, commits)| {
        let pb = pb.clone();
        let model = Arc::clone(&model);
        async move {
            // Resolve all descriptions for this day concurrently
            let desc_futs = commits.iter().map(|commit| {
                let model = Arc::clone(&model);
                let pb = pb.clone();
                let hash = commit.short_hash.clone();
                let body = commit.body.clone();
                let repo_path = commit.repo_path.clone();
                async move {
                    if !body.is_empty() {
                        return Some(body);
                    }
                    let Some(ref m) = *model else { return None; };
                    pb.set_message(format!("LLM: {hash}"));
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
            if let Err(e) = fs::create_dir_all(log_path.parent().unwrap()) {
                error!("failed to create dir for {}: {e}", log_path.display());
                pb.inc(commits.len() as u64);
                return;
            }
            match File::create(&log_path) {
                Ok(mut file) => {
                    for (commit, desc) in commits.iter().zip(descriptions) {
                        let dt = Local
                            .timestamp_opt(commit.timestamp, 0)
                            .single()
                            .unwrap_or_else(|| Local::now());
                        let time_str = dt.format("%H:%M:%S").to_string();
                        if let Err(e) = write_entry(&mut file, &commit.short_hash, &commit.title, desc.as_deref(), &commit.repo, &time_str) {
                            error!("failed to write entry {}: {e}", commit.short_hash);
                        }
                        pb.inc(1);
                    }
                    info!("wrote {} commits to {}", commits.len(), log_path.display());
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

    println!(
        "Wrote {} days of logs to {}",
        num_days,
        remi_dir().display()
    );

    if let Some(model) = load_config().model {
        maybe_generate_recaps(&model).await;
    }
}

// ── Recaps ────────────────────────────────────────────────────────────────────

/// Calls the LLM to summarize `content` into a recap. `period` is used in the prompt ("month" / "year").
async fn llm_recap(content: &str, period: &str, model: &str) -> Option<String> {
    use genai::chat::{ChatMessage, ChatRequest};
    use genai::Client;

    const MAX_CHARS: usize = 40_000;
    let content = if content.len() > MAX_CHARS {
        warn!("recap content truncated to {MAX_CHARS} chars");
        &content[..MAX_CHARS]
    } else {
        content
    };

    let prompt = format!(
        "The following is a log of all git commits I made this {period}, \
         including their descriptions. Write a concise but thorough recap of \
         what I worked on: key themes, notable achievements, and any recurring \
         projects or areas of focus. Use markdown with bullet points.\n\n{content}"
    );

    let req = ChatRequest::new(vec![ChatMessage::user(prompt)]);
    match Client::default().exec_chat(model, req, None).await {
        Ok(r) => {
            let text = r.content_text_into_string();
            if text.is_none() {
                warn!("LLM returned empty recap");
            }
            text
        }
        Err(e) => {
            error!("LLM recap call failed: {e}");
            None
        }
    }
}

fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap() - chrono::Duration::days(1)
}

async fn generate_month_recap(month_path: &Path, model: &str) {
    let recap_path = month_path.join("recap.md");

    // Collect daily logs sorted by filename
    let Ok(entries) = fs::read_dir(month_path) else { return };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "md").unwrap_or(false)
                && p.file_name().map(|n| n != "recap.md").unwrap_or(false)
        })
        .collect();
    files.sort();

    if files.is_empty() {
        return;
    }

    let mut content = String::new();
    for file in &files {
        if let Ok(text) = fs::read_to_string(file) {
            content.push_str(&text);
            content.push('\n');
        }
    }

    let period_label = month_path
        .components()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");

    info!("generating month recap for {period_label}");
    if let Some(recap) = llm_recap(&content, "month", model).await {
        if let Err(e) = fs::write(&recap_path, recap) {
            error!("failed to write month recap: {e}");
        } else {
            info!("wrote month recap to {}", recap_path.display());
        }
    }
}

async fn generate_year_recap(year_path: &Path, model: &str) {
    let recap_path = year_path.join("recap.md");

    // Collect monthly recap.md files sorted by month
    let Ok(entries) = fs::read_dir(year_path) else { return };
    let mut month_recaps: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|p| p.join("recap.md"))
        .filter(|p| p.exists())
        .collect();
    month_recaps.sort();

    if month_recaps.is_empty() {
        return;
    }

    let mut content = String::new();
    for file in &month_recaps {
        // Add a header indicating which month this recap is from
        if let Some(month) = file.parent().and_then(|p| p.file_name()) {
            content.push_str(&format!("## Month {}\n\n", month.to_string_lossy()));
        }
        if let Ok(text) = fs::read_to_string(file) {
            content.push_str(&text);
            content.push('\n');
        }
    }

    let year_label = year_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    info!("generating year recap for {year_label}");
    if let Some(recap) = llm_recap(&content, "year", model).await {
        if let Err(e) = fs::write(&recap_path, recap) {
            error!("failed to write year recap: {e}");
        } else {
            info!("wrote year recap to {}", recap_path.display());
        }
    }
}

/// Collects (year, month, path) tuples for all past month directories under remi_dir.
fn past_month_dirs(
    today: NaiveDate,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
) -> Vec<(i32, u32, PathBuf)> {
    let mut result = Vec::new();
    let Ok(year_entries) = fs::read_dir(remi_dir()) else { return result };
    for ye in year_entries.flatten() {
        let year_path = ye.path();
        if !year_path.is_dir() { continue; }
        let Ok(year_num) = ye.file_name().to_string_lossy().parse::<i32>() else { continue; };
        let Ok(month_entries) = fs::read_dir(&year_path) else { continue; };
        for me in month_entries.flatten() {
            let month_path = me.path();
            if !month_path.is_dir() { continue; }
            let Ok(month_num) = me.file_name().to_string_lossy().parse::<u32>() else { continue; };
            let first_day = NaiveDate::from_ymd_opt(year_num, month_num, 1).unwrap();
            let last_day = last_day_of_month(year_num, month_num);
            // Month must be fully in the past
            if last_day >= today { continue; }
            // start must not cut into the middle of the month
            if start.map(|s| s > first_day).unwrap_or(false) { continue; }
            // end must not cut into the middle of the month
            if end.map(|e| e < last_day).unwrap_or(false) { continue; }
            result.push((year_num, month_num, month_path));
        }
    }
    result
}

/// Collects year paths for all past year directories under remi_dir.
fn past_year_dirs(
    today: NaiveDate,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
) -> Vec<(i32, PathBuf)> {
    let mut result = Vec::new();
    let Ok(year_entries) = fs::read_dir(remi_dir()) else { return result };
    for ye in year_entries.flatten() {
        let year_path = ye.path();
        if !year_path.is_dir() { continue; }
        let Ok(year_num) = ye.file_name().to_string_lossy().parse::<i32>() else { continue; };
        let year_first = NaiveDate::from_ymd_opt(year_num, 1, 1).unwrap();
        let year_last = NaiveDate::from_ymd_opt(year_num, 12, 31).unwrap();
        if year_last >= today { continue; }
        if start.map(|s| s > year_first).unwrap_or(false) { continue; }
        if end.map(|e| e < year_last).unwrap_or(false) { continue; }
        result.push((year_num, year_path));
    }
    result
}

/// Generates any missing recaps for past months and years. Skips existing recap.md files.
async fn maybe_generate_recaps(model: &str) {
    let today = Local::now().date_naive();

    // Months first (year recaps depend on them)
    let month_tasks: Vec<_> = past_month_dirs(today, None, None)
        .into_iter()
        .filter(|(_, _, p)| !p.join("recap.md").exists())
        .map(|(_, _, path)| {
            let model = model.to_string();
            tokio::spawn(async move { generate_month_recap(&path, &model).await })
        })
        .collect();
    for t in month_tasks { if let Err(e) = t.await { error!("month recap task panicked: {e}"); } }

    // Years after months are done
    let year_tasks: Vec<_> = past_year_dirs(today, None, None)
        .into_iter()
        .filter(|(_, p)| !p.join("recap.md").exists())
        .map(|(_, path)| {
            let model = model.to_string();
            tokio::spawn(async move { generate_year_recap(&path, &model).await })
        })
        .collect();
    for t in year_tasks { if let Err(e) = t.await { error!("year recap task panicked: {e}"); } }
}

/// Regenerates recaps (overwriting existing) for all complete months/years within the date range.
async fn run_recap(start: Option<String>, end: Option<String>) {
    let Some(model) = load_config().model else {
        eprintln!("remi: no model configured in ~/.remi/config.toml");
        return;
    };

    let start_date = start.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let end_date   = end.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let today = Local::now().date_naive();

    let months = past_month_dirs(today, start_date, end_date);
    let years  = past_year_dirs(today, start_date, end_date);

    let total = months.len() + years.len();
    if total == 0 {
        println!("No complete periods found to recap.");
        return;
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len}  {msg}",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    // Months first (in parallel), then years
    let month_tasks: Vec<_> = months
        .into_iter()
        .map(|(y, m, path)| {
            let model = model.clone();
            let pb = pb.clone();
            tokio::spawn(async move {
                pb.set_message(format!("month {y}/{m:02}"));
                generate_month_recap(&path, &model).await;
                pb.inc(1);
            })
        })
        .collect();
    for t in month_tasks { if let Err(e) = t.await { error!("month recap task panicked: {e}"); } }

    let year_tasks: Vec<_> = years
        .into_iter()
        .map(|(y, path)| {
            let model = model.clone();
            let pb = pb.clone();
            tokio::spawn(async move {
                pb.set_message(format!("year {y}"));
                generate_year_recap(&path, &model).await;
                pb.inc(1);
            })
        })
        .collect();
    for t in year_tasks { if let Err(e) = t.await { error!("year recap task panicked: {e}"); } }

    pb.finish_with_message("done");
}

// ── Logger ────────────────────────────────────────────────────────────────────

fn init_logger() {
    let log_path = remi_dir().join("remi.log");
    if let Ok(()) = fs::create_dir_all(remi_dir()) {
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = WriteLogger::init(LevelFilter::Info, LogConfig::default(), file);
        }
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

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
        Some(Commands::Recap { start, end }) => {
            run_recap(start, end).await;
        }
        Some(Commands::Scan { path, start, end }) => {
            run_scan(path, start, end).await;
        }
        None => {
            ensure_hook();
            info!("commits will be logged under {}", remi_dir().display());
            println!("remi: commits will be logged under {}", remi_dir().display());
        }
    }
}
