use std::{fs, path::Path};

use chrono::{Local, NaiveDate};
use log::error;

use crate::paths::remi_dir;
use crate::pending::write_pending_recap;

pub fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap() - chrono::Duration::days(1)
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
            if last_day >= today {
                continue;
            }
            if start.map(|s| s > first_day).unwrap_or(false) {
                continue;
            }
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

fn queue_month_recap(month_path: &Path) {
    let recap_path = month_path.join("recap.md");

    // Check there are daily logs to recap
    let Ok(entries) = fs::read_dir(month_path) else { return };
    let has_logs = entries
        .flatten()
        .any(|e| e.path().extension().map(|x| x == "md").unwrap_or(false) && e.file_name() != "recap.md");
    if !has_logs {
        return;
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
        .join("-");

    if let Err(e) = write_pending_recap(&period_label, "month", &recap_path.to_path_buf()) {
        error!("failed to write pending recap for {period_label}: {e}");
    } else {
        log::info!("queued month recap for {period_label}");
    }
}

fn queue_year_recap(year_path: &Path) {
    let recap_path = year_path.join("recap.md");

    // Check there are month recaps or daily logs to draw from
    let Ok(entries) = fs::read_dir(year_path) else { return };
    let has_months = entries.flatten().any(|e| e.path().is_dir());
    if !has_months {
        return;
    }

    let year_label = year_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    if let Err(e) = write_pending_recap(&year_label, "year", &recap_path.to_path_buf()) {
        error!("failed to write pending recap for {year_label}: {e}");
    } else {
        log::info!("queued year recap for {year_label}");
    }
}

/// Queues pending recap files for any past months/years that don't have recap.md yet.
pub fn maybe_generate_recaps() {
    let today = Local::now().date_naive();

    for (_, _, path) in past_month_dirs(today, None, None) {
        if !path.join("recap.md").exists() {
            queue_month_recap(&path);
        }
    }

    for (_, path) in past_year_dirs(today, None, None) {
        if !path.join("recap.md").exists() {
            queue_year_recap(&path);
        }
    }
}

/// Queues pending recap files for all complete months/years within the date range.
pub fn run_recap(start: Option<String>, end: Option<String>) {
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

    let mut queued = 0;
    for (_, _, path) in months {
        queue_month_recap(&path);
        queued += 1;
    }
    for (_, path) in years {
        queue_year_recap(&path);
        queued += 1;
    }

    println!("Queued {queued} recap(s) in {}. Run `remi check` to process them.", crate::paths::pending_dir().display());
}
