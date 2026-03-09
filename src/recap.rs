use std::{fs, path::Path};

use chrono::{Local, NaiveDate};
use indicatif::{ProgressBar, ProgressStyle};
use log::error;

use crate::config::load_config;
use crate::llm::llm_recap;
use crate::paths::remi_dir;

pub fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap() - chrono::Duration::days(1)
}

pub async fn generate_month_recap(month_path: &Path, model: &str) {
    let recap_path = month_path.join("recap.md");

    // Collect daily logs sorted by filename
    let Ok(entries) = fs::read_dir(month_path) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false) && p.file_name().map(|n| n != "recap.md").unwrap_or(false))
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

    log::info!("generating month recap for {period_label}");
    if let Some(recap) = llm_recap(&content, "month", model).await {
        if let Err(e) = fs::write(&recap_path, recap) {
            error!("failed to write month recap: {e}");
        } else {
            log::info!("wrote month recap to {}", recap_path.display());
        }
    }
}

pub async fn generate_year_recap(year_path: &Path, model: &str) {
    let recap_path = year_path.join("recap.md");

    // Collect monthly recap.md files sorted by month
    let Ok(entries) = fs::read_dir(year_path) else { return };
    let mut month_recaps: Vec<_> = entries
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
        if let Some(month) = file.parent().and_then(|p| p.file_name()) {
            content.push_str(&format!("## Month {}\n\n", month.to_string_lossy()));
        }
        if let Ok(text) = fs::read_to_string(file) {
            content.push_str(&text);
            content.push('\n');
        }
    }

    let year_label = year_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    log::info!("generating year recap for {year_label}");
    if let Some(recap) = llm_recap(&content, "year", model).await {
        if let Err(e) = fs::write(&recap_path, recap) {
            error!("failed to write year recap: {e}");
        } else {
            log::info!("wrote year recap to {}", recap_path.display());
        }
    }
}

/// Collects (year, month, path) tuples for all past month directories under remi_dir.
pub fn past_month_dirs(today: NaiveDate, start: Option<NaiveDate>, end: Option<NaiveDate>) -> Vec<(i32, u32, std::path::PathBuf)> {
    let mut result = Vec::new();
    let Ok(year_entries) = fs::read_dir(remi_dir()) else {
        return result;
    };
    for ye in year_entries.flatten() {
        let year_path = ye.path();
        if !year_path.is_dir() {
            continue;
        }
        let Ok(year_num) = ye.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(month_entries) = fs::read_dir(&year_path) else {
            continue;
        };
        for me in month_entries.flatten() {
            let month_path = me.path();
            if !month_path.is_dir() {
                continue;
            }
            let Ok(month_num) = me.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let first_day = NaiveDate::from_ymd_opt(year_num, month_num, 1).unwrap();
            let last_day = last_day_of_month(year_num, month_num);
            // Month must be fully in the past
            if last_day >= today {
                continue;
            }
            // start must not cut into the middle of the month
            if start.map(|s| s > first_day).unwrap_or(false) {
                continue;
            }
            // end must not cut into the middle of the month
            if end.map(|e| e < last_day).unwrap_or(false) {
                continue;
            }
            result.push((year_num, month_num, month_path));
        }
    }
    result
}

/// Collects year paths for all past year directories under remi_dir.
pub fn past_year_dirs(today: NaiveDate, start: Option<NaiveDate>, end: Option<NaiveDate>) -> Vec<(i32, std::path::PathBuf)> {
    let mut result = Vec::new();
    let Ok(year_entries) = fs::read_dir(remi_dir()) else {
        return result;
    };
    for ye in year_entries.flatten() {
        let year_path = ye.path();
        if !year_path.is_dir() {
            continue;
        }
        let Ok(year_num) = ye.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let year_first = NaiveDate::from_ymd_opt(year_num, 1, 1).unwrap();
        let year_last = NaiveDate::from_ymd_opt(year_num, 12, 31).unwrap();
        if year_last >= today {
            continue;
        }
        if start.map(|s| s > year_first).unwrap_or(false) {
            continue;
        }
        if end.map(|e| e < year_last).unwrap_or(false) {
            continue;
        }
        result.push((year_num, year_path));
    }
    result
}

/// Generates any missing recaps for past months and years. Skips existing recap.md files.
pub async fn maybe_generate_recaps(model: &str) {
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
    for t in month_tasks {
        if let Err(e) = t.await {
            error!("month recap task panicked: {e}");
        }
    }

    // Years after months are done
    let year_tasks: Vec<_> = past_year_dirs(today, None, None)
        .into_iter()
        .filter(|(_, p)| !p.join("recap.md").exists())
        .map(|(_, path)| {
            let model = model.to_string();
            tokio::spawn(async move { generate_year_recap(&path, &model).await })
        })
        .collect();
    for t in year_tasks {
        if let Err(e) = t.await {
            error!("year recap task panicked: {e}");
        }
    }
}

/// Regenerates recaps (overwriting existing) for all complete months/years within the date range.
pub async fn run_recap(start: Option<String>, end: Option<String>) {
    let model = load_config().model().to_owned();

    let start_date = start.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let end_date = end.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let today = Local::now().date_naive();

    let months = past_month_dirs(today, start_date, end_date);
    let years = past_year_dirs(today, start_date, end_date);

    let total = months.len() + years.len();
    if total == 0 {
        println!("No complete periods found to recap.");
        return;
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len}  {msg}")
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
    for t in month_tasks {
        if let Err(e) = t.await {
            error!("month recap task panicked: {e}");
        }
    }

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
    for t in year_tasks {
        if let Err(e) = t.await {
            error!("year recap task panicked: {e}");
        }
    }

    pb.finish_with_message("done");
}
