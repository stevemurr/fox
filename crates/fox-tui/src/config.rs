//! Configuration management

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// General settings
    #[serde(default)]
    pub general: GeneralConfig,

    /// Display settings
    #[serde(default)]
    pub display: DisplayConfig,

    /// Keybindings (custom overrides)
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Default mode: "reader" or "full"
    #[serde(default = "default_mode")]
    pub default_mode: String,

    /// Enable JavaScript rendering
    #[serde(default = "default_true")]
    pub javascript: bool,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_mode: default_mode(),
            javascript: default_true(),
            timeout_secs: default_timeout(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// Maximum text width for wrapping
    #[serde(default = "default_width")]
    pub max_width: usize,

    /// Link display style: "inline", "footnote", "hidden"
    #[serde(default = "default_link_style")]
    pub show_links: String,

    /// Show images as placeholders
    #[serde(default = "default_true")]
    pub show_images: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            max_width: default_width(),
            show_links: default_link_style(),
            show_images: default_true(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeybindingsConfig {
    /// Custom keybindings can be added here
    #[serde(flatten)]
    pub custom: std::collections::HashMap<String, String>,
}

// Default value functions
fn default_mode() -> String {
    "reader".to_string()
}
fn default_true() -> bool {
    true
}
fn default_timeout() -> u64 {
    30
}
fn default_width() -> usize {
    80
}
fn default_link_style() -> String {
    "inline".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            display: DisplayConfig::default(),
            keybindings: KeybindingsConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from file or use defaults
    pub fn load() -> Result<Self> {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                let content = fs::read_to_string(&path)?;
                let config: Config = toml::from_str(&content)?;
                return Ok(config);
            }
        }
        Ok(Self::default())
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(self)?;
            fs::write(path, content)?;
        }
        Ok(())
    }

    fn config_path() -> Option<PathBuf> {
        ProjectDirs::from("com", "fox", "fox")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    /// Whether JavaScript is enabled
    pub fn javascript(&self) -> bool {
        self.general.javascript
    }

    /// Set a configuration value
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "javascript" | "js" => {
                self.general.javascript = value.parse().unwrap_or(true);
            }
            "max_width" | "width" => {
                self.display.max_width = value.parse().unwrap_or(80);
            }
            "show_links" | "links" => {
                self.display.show_links = value.to_string();
            }
            "timeout" => {
                self.general.timeout_secs = value.parse().unwrap_or(30);
            }
            _ => {
                // Store in custom keybindings
                self.keybindings.custom.insert(key.to_string(), value.to_string());
            }
        }
        self.save()?;
        Ok(())
    }
}

// Expose javascript as a direct field for convenience
impl std::ops::Deref for Config {
    type Target = GeneralConfig;

    fn deref(&self) -> &Self::Target {
        &self.general
    }
}
