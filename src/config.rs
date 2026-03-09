use std::fs;

use serde::Deserialize;

use crate::paths::remi_dir;

#[derive(Deserialize, Default)]
pub struct Config {
    pub model: Option<String>,
}

impl Config {
    pub fn model(&self) -> &str {
        self.model.as_deref().unwrap_or("gemini-3.1-flash-lite-preview")
    }
}

pub fn load_config() -> Config {
    let path = remi_dir().join("config.toml");
    let Ok(contents) = fs::read_to_string(&path) else {
        return Config::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}
