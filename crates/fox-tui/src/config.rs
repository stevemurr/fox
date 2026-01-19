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

    /// Browser settings
    #[serde(default)]
    pub browser: BrowserConfig,

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

/// Browser/Chrome settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Browser mode: "auto", "bundled", "system", or "none"
    /// - auto: Use bundled Chrome if available, fall back to system, then download
    /// - bundled: Use downloaded Chrome for Testing only
    /// - system: Use system-installed Chrome only
    /// - none: Disable JavaScript rendering entirely
    #[serde(default = "default_browser_mode")]
    pub mode: String,

    /// Custom Chrome binary path (only used when mode = "system")
    #[serde(default)]
    pub chrome_path: Option<String>,

    /// Auto-update bundled Chrome when a new version is available
    #[serde(default = "default_true")]
    pub auto_update: bool,

    /// Content extraction method: "accessibility" or "readability"
    /// - accessibility: Use Chrome's accessibility tree (requires JS, better for dynamic pages)
    /// - readability: Use readability-style extraction (works without JS)
    #[serde(default = "default_extraction_method")]
    pub extraction_method: String,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            mode: default_browser_mode(),
            chrome_path: None,
            auto_update: default_true(),
            extraction_method: default_extraction_method(),
        }
    }
}

fn default_browser_mode() -> String {
    "auto".to_string()
}

fn default_extraction_method() -> String {
    "accessibility".to_string()
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
            browser: BrowserConfig::default(),
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
            "browser_mode" | "browser" => {
                let valid = ["auto", "bundled", "system", "none"];
                if valid.contains(&value) {
                    self.browser.mode = value.to_string();
                }
            }
            "chrome_path" => {
                self.browser.chrome_path = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
            }
            "auto_update" => {
                self.browser.auto_update = value.parse().unwrap_or(true);
            }
            "extraction_method" | "extraction" => {
                let valid = ["accessibility", "ax", "a11y", "readability", "reader"];
                if valid.contains(&value.to_lowercase().as_str()) {
                    self.browser.extraction_method = match value.to_lowercase().as_str() {
                        "accessibility" | "ax" | "a11y" => "accessibility".to_string(),
                        "readability" | "reader" => "readability".to_string(),
                        _ => "accessibility".to_string(),
                    };
                }
            }
            _ => {
                // Store in custom keybindings
                self.keybindings.custom.insert(key.to_string(), value.to_string());
            }
        }
        self.save()?;
        Ok(())
    }

    /// Convert to fox-core ChromeConfig
    pub fn to_chrome_config(&self) -> fox_core::ChromeConfig {
        let data_dir = ProjectDirs::from("", "", "fox")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from(".fox"));

        let extraction_method = self.browser.extraction_method.parse()
            .unwrap_or(fox_core::ExtractionMethod::Accessibility);

        fox_core::ChromeConfig {
            mode: self.browser.mode.clone(),
            chrome_path: self.browser.chrome_path.as_ref().map(std::path::PathBuf::from),
            data_dir,
            auto_update: self.browser.auto_update,
            extraction_method,
        }
    }
}

// Expose javascript as a direct field for convenience
impl std::ops::Deref for Config {
    type Target = GeneralConfig;

    fn deref(&self) -> &Self::Target {
        &self.general
    }
}
