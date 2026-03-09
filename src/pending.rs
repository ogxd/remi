use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use chrono::{Local, TimeZone};
use crate::journal::write_entry;
use crate::paths::{daily_log_file_for, pending_dir};

pub fn write_pending_commit(hash: &str, repo: &str, title: &str, timestamp: i64, diff: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = pending_dir();
    fs::create_dir_all(&dir)?;
    let content = format!("type: commit\nhash: {hash}\nrepo: {repo}\ntimestamp: {timestamp}\ntitle: {title}\n===\n{diff}");
    fs::write(dir.join(format!("{hash}.md")), content)?;
    Ok(())
}

pub fn write_pending_recap(period: &str, period_type: &str, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dir = pending_dir();
    fs::create_dir_all(&dir)?;
    let content = format!("type: recap\nperiod: {period}\nperiod_type: {period_type}\noutput: {}\n", output.display());
    fs::write(dir.join(format!("recap-{period}.md")), content)?;
    Ok(())
}

struct PendingCommit {
    hash: String,
    repo: String,
    title: String,
    timestamp: i64,
    diff: String,
}

struct PendingRecap {
    period: String,
    period_type: String,
    output: PathBuf,
}

enum PendingItem {
    Commit(PendingCommit),
    Recap(PendingRecap),
}

fn parse_pending_file(path: &PathBuf) -> Option<PendingItem> {
    let content = fs::read_to_string(path).ok()?;

    let (header, diff) = if let Some(idx) = content.find("\n===\n") {
        (&content[..idx], &content[idx + 5..])
    } else {
        (content.as_str(), "")
    };

    let mut item_type = String::new();
    let mut hash = String::new();
    let mut repo = String::new();
    let mut title = String::new();
    let mut timestamp: i64 = 0;
    let mut period = String::new();
    let mut period_type = String::new();
    let mut output = String::new();

    for line in header.lines() {
        if let Some(val) = line.strip_prefix("type: ") {
            item_type = val.to_string();
        } else if let Some(val) = line.strip_prefix("hash: ") {
            hash = val.to_string();
        } else if let Some(val) = line.strip_prefix("repo: ") {
            repo = val.to_string();
        } else if let Some(val) = line.strip_prefix("title: ") {
            title = val.to_string();
        } else if let Some(val) = line.strip_prefix("timestamp: ") {
            timestamp = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("period: ") {
            period = val.to_string();
        } else if let Some(val) = line.strip_prefix("period_type: ") {
            period_type = val.to_string();
        } else if let Some(val) = line.strip_prefix("output: ") {
            output = val.to_string();
        }
    }

    match item_type.as_str() {
        "commit" => Some(PendingItem::Commit(PendingCommit { hash, repo, title, timestamp, diff: diff.to_string() })),
        "recap" => Some(PendingItem::Recap(PendingRecap { period, period_type, output: PathBuf::from(output) })),
        _ => None,
    }
}

fn read_month_log_content(month_dir: &PathBuf) -> String {
    let Ok(entries) = fs::read_dir(month_dir) else { return String::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false) && p.file_name().map(|n| n != "recap.md").unwrap_or(false))
        .collect();
    files.sort();
    let mut content = String::new();
    for file in files {
        if let Ok(text) = fs::read_to_string(&file) {
            content.push_str(&text);
            content.push('\n');
        }
    }
    content
}

fn read_year_log_content(year_dir: &PathBuf) -> String {
    let Ok(entries) = fs::read_dir(year_dir) else { return String::new() };
    let mut month_dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    month_dirs.sort();
    let mut content = String::new();
    for month_dir in month_dirs {
        let recap = month_dir.join("recap.md");
        if recap.exists() {
            if let Some(month) = month_dir.file_name() {
                content.push_str(&format!("## Month {}\n\n", month.to_string_lossy()));
            }
            if let Ok(text) = fs::read_to_string(&recap) {
                content.push_str(&text);
                content.push('\n');
            }
        } else {
            // Fall back to daily logs if monthly recap not yet generated
            let daily = read_month_log_content(&month_dir);
            if !daily.is_empty() {
                if let Some(month) = month_dir.file_name() {
                    content.push_str(&format!("## Month {}\n\n", month.to_string_lossy()));
                }
                content.push_str(&daily);
            }
        }
    }
    content
}

pub fn run_check() {
    let dir = pending_dir();
    if !dir.exists() {
        println!("No pending items.");
        return;
    }

    let Ok(entries) = fs::read_dir(&dir) else {
        println!("No pending items.");
        return;
    };

    let paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();

    let mut commits: Vec<PendingCommit> = Vec::new();
    let mut month_recaps: Vec<PendingRecap> = Vec::new();
    let mut year_recaps: Vec<PendingRecap> = Vec::new();

    for path in &paths {
        match parse_pending_file(path) {
            Some(PendingItem::Commit(c)) => commits.push(c),
            Some(PendingItem::Recap(r)) => {
                if r.period_type == "year" {
                    year_recaps.push(r);
                } else {
                    month_recaps.push(r);
                }
            }
            None => {}
        }
    }

    commits.sort_by_key(|c| c.timestamp);
    month_recaps.sort_by(|a, b| a.period.cmp(&b.period));
    year_recaps.sort_by(|a, b| a.period.cmp(&b.period));

    if commits.is_empty() && month_recaps.is_empty() && year_recaps.is_empty() {
        println!("No pending items.");
        return;
    }

    for c in commits {
        println!("PENDING COMMIT");
        println!("hash: {}", c.hash);
        println!("repo: {}", c.repo);
        println!("title: {}", c.title);
        println!("timestamp: {}", c.timestamp);
        println!("diff:");
        println!("{}", c.diff);
        println!("action: Summarize the diff above in one sentence and run: remi record commit {} \"<your summary>\"", c.hash);
        println!();
    }

    for r in month_recaps {
        let log = if let Some(month_dir) = r.output.parent() {
            read_month_log_content(&month_dir.to_path_buf())
        } else {
            String::new()
        };
        println!("PENDING RECAP");
        println!("period: {}", r.period);
        println!("period_type: {}", r.period_type);
        println!("output: {}", r.output.display());
        println!("log:");
        println!("{log}");
        println!("action: Generate a recap from the log above, write it to {}, then run: remi record recap {}", r.output.display(), r.period);
        println!();
    }

    for r in year_recaps {
        let log = if let Some(year_dir) = r.output.parent() {
            read_year_log_content(&year_dir.to_path_buf())
        } else {
            String::new()
        };
        println!("PENDING RECAP");
        println!("period: {}", r.period);
        println!("period_type: {}", r.period_type);
        println!("output: {}", r.output.display());
        println!("log:");
        println!("{log}");
        println!("action: Generate a recap from the log above, write it to {}, then run: remi record recap {}", r.output.display(), r.period);
        println!();
    }
}

pub fn record_commit(hash: &str, summary: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pending_path = pending_dir().join(format!("{hash}.md"));
    let Some(item) = parse_pending_file(&pending_path) else {
        return Err(format!("No pending commit found for hash {hash}").into());
    };
    let PendingItem::Commit(c) = item else {
        return Err(format!("Pending file for {hash} is not a commit").into());
    };

    let dt = Local.timestamp_opt(c.timestamp, 0).single().unwrap_or_else(Local::now);
    let date = dt.date_naive();
    let time_str = dt.format("%H:%M:%S").to_string();

    let log_path = daily_log_file_for(&date);
    fs::create_dir_all(log_path.parent().unwrap())?;

    let mut file = OpenOptions::new().create(true).append(true).open(&log_path)?;
    write_entry(&mut file, hash, &c.title, Some(summary), &c.repo, &time_str)?;

    fs::remove_file(&pending_path)?;

    log::info!("recorded commit [{hash}] in {}", log_path.display());
    Ok(())
}

pub fn record_recap(period: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pending_path = pending_dir().join(format!("recap-{period}.md"));
    if !pending_path.exists() {
        return Err(format!("No pending recap found for period {period}").into());
    }
    fs::remove_file(&pending_path)?;
    log::info!("marked recap for {period} as done");
    Ok(())
}
