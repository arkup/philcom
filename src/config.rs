use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub buttons: Vec<Button>,
    #[serde(default)]
    pub bookmarks: Vec<String>,
    #[serde(default)]
    pub restore_session: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Button {
    pub key: u8,
    pub label: String,
    pub command: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            buttons: default_buttons(),
            bookmarks: Vec::new(),
            restore_session: false,
        }
    }
}

// ── Session ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    pub left_tabs:    Vec<String>,
    pub left_active:  usize,
    pub right_tabs:   Vec<String>,
    pub right_active: usize,
    pub active_panel: String,
}

impl Session {
    pub fn session_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("philcom").join("session.toml")
    }

    pub fn load() -> Option<Self> {
        let path = Self::session_path();
        if path.exists() {
            let content = fs::read_to_string(&path).ok()?;
            toml::from_str(&content).ok()
        } else {
            None
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn default_buttons() -> Vec<Button> {
    vec![
        Button { key: 1,  label: "History".to_string(), command: "history".to_string() },
        Button { key: 2,  label: "Menu".to_string(),   command: "menu".to_string() },
        Button { key: 3,  label: "View".to_string(),   command: "view".to_string() },
        Button { key: 4,  label: "Edit".to_string(),   command: "edit".to_string() },
        Button { key: 5,  label: "Copy".to_string(),   command: "copy".to_string() },
        Button { key: 6,  label: "Move".to_string(),   command: "move".to_string() },
        Button { key: 7,  label: "Mkdir".to_string(),  command: "mkdir".to_string() },
        Button { key: 8,  label: "Delete".to_string(), command: "delete".to_string() },
        Button { key: 9,  label: "Bmarks".to_string(), command: "bookmark".to_string() },
        Button { key: 10, label: "Quit".to_string(),   command: "quit".to_string() },
    ]
}

impl Config {
    pub fn config_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("philcom").join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}
