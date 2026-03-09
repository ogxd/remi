use std::path::PathBuf;

use chrono::NaiveDate;

pub fn remi_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".remi")
}

pub fn hooks_dir() -> PathBuf {
    remi_dir().join("hooks")
}

pub fn daily_log_file_for(date: &NaiveDate) -> PathBuf {
    let year = date.format("%Y").to_string();
    let month = date.format("%m").to_string();
    let filename = date.format("%d-%m-%Y.md").to_string();
    remi_dir().join(year).join(month).join(filename)
}

pub fn hook_script_path() -> PathBuf {
    hooks_dir().join("post-commit")
}

pub fn pending_dir() -> PathBuf {
    remi_dir().join("pending")
}
