//! Navigation history management

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: Option<String>,
    pub timestamp: u64,
}

/// Navigation history
#[derive(Debug, Default)]
pub struct History {
    /// All history entries
    entries: Vec<HistoryEntry>,
    /// Current position in the back/forward stack
    position: usize,
    /// Back/forward stack for current session
    session_stack: Vec<String>,
    /// Maximum entries to keep
    max_entries: usize,
}

impl History {
    /// Create a new empty history
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            position: 0,
            session_stack: Vec::new(),
            max_entries: 10000,
        }
    }

    /// Load history from disk
    pub fn load() -> Result<Self> {
        let mut history = Self::new();

        if let Some(path) = Self::history_path() {
            if path.exists() {
                let content = fs::read_to_string(&path)?;
                history.entries = serde_json::from_str(&content).unwrap_or_default();
            }
        }

        Ok(history)
    }

    /// Save history to disk
    pub fn save(&self) -> Result<()> {
        if let Some(path) = Self::history_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = serde_json::to_string_pretty(&self.entries)?;
            fs::write(path, content)?;
        }
        Ok(())
    }

    fn history_path() -> Option<PathBuf> {
        ProjectDirs::from("com", "fox", "fox")
            .map(|dirs| dirs.data_dir().join("history.json"))
    }

    /// Add a new URL to history
    pub fn add(&mut self, url: &str, title: Option<&str>) {
        let entry = HistoryEntry {
            url: url.to_string(),
            title: title.map(String::from),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        // Add to persistent history
        self.entries.push(entry);

        // Trim if too long
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }

        // Add to session stack
        // If we're not at the end of the stack, truncate
        if self.position < self.session_stack.len() {
            self.session_stack.truncate(self.position);
        }
        self.session_stack.push(url.to_string());
        self.position = self.session_stack.len();

        // Auto-save
        let _ = self.save();
    }

    /// Go back in history
    pub fn back(&mut self) -> Option<String> {
        if self.position > 1 {
            self.position -= 1;
            self.session_stack.get(self.position - 1).cloned()
        } else {
            None
        }
    }

    /// Go forward in history
    pub fn forward(&mut self) -> Option<String> {
        if self.position < self.session_stack.len() {
            self.position += 1;
            self.session_stack.get(self.position - 1).cloned()
        } else {
            None
        }
    }

    /// Check if we can go back
    pub fn can_go_back(&self) -> bool {
        self.position > 1
    }

    /// Check if we can go forward
    pub fn can_go_forward(&self) -> bool {
        self.position < self.session_stack.len()
    }

    /// Get recent history entries
    pub fn recent(&self, count: usize) -> Vec<&HistoryEntry> {
        self.entries.iter().rev().take(count).collect()
    }

    /// List history as a string
    pub fn list(&self) -> String {
        self.recent(10)
            .iter()
            .map(|e| {
                let title = e.title.as_deref().unwrap_or(&e.url);
                format!("{}", title)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Search history
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.url.to_lowercase().contains(&query_lower)
                    || e.title
                        .as_ref()
                        .map(|t| t.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Clear all history
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.session_stack.clear();
        self.position = 0;
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_back_forward() {
        let mut history = History::new();

        history.add("https://a.com", Some("A"));
        history.add("https://b.com", Some("B"));
        history.add("https://c.com", Some("C"));

        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        assert_eq!(history.back(), Some("https://b.com".to_string()));
        assert!(history.can_go_forward());

        assert_eq!(history.back(), Some("https://a.com".to_string()));
        assert!(!history.can_go_back());

        assert_eq!(history.forward(), Some("https://b.com".to_string()));
        assert_eq!(history.forward(), Some("https://c.com".to_string()));
        assert!(!history.can_go_forward());
    }

    #[test]
    fn test_history_search() {
        let mut history = History::new();

        history.add("https://rust-lang.org", Some("Rust"));
        history.add("https://github.com", Some("GitHub"));
        history.add("https://docs.rs", Some("Rust Docs"));

        let results = history.search("rust");
        assert_eq!(results.len(), 2);
    }
}
