//! Vim mode handling

/// Current vim mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    /// Normal navigation mode
    Normal,
    /// Command mode (: prefix)
    Command,
    /// Search mode (/ prefix)
    Search,
    /// Insert mode for forms
    Insert,
    /// Link hint mode (f key)
    Hint,
}

impl Default for VimMode {
    fn default() -> Self {
        Self::Normal
    }
}

impl VimMode {
    /// Get the mode indicator string
    pub fn indicator(&self) -> &'static str {
        match self {
            VimMode::Normal => "NORMAL",
            VimMode::Command => "COMMAND",
            VimMode::Search => "SEARCH",
            VimMode::Insert => "INSERT",
            VimMode::Hint => "HINT",
        }
    }
}

/// Vim state
#[derive(Debug, Default)]
pub struct VimState {
    /// Current mode
    pub mode: VimMode,
    /// Numeric prefix for commands (e.g., 5j)
    pub count: Option<usize>,
    /// Register for yank/paste
    pub register: Option<String>,
}

impl VimState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset to normal mode
    pub fn reset(&mut self) {
        self.mode = VimMode::Normal;
        self.count = None;
    }
}

/// Parsed command from command mode
#[derive(Debug, Clone)]
pub enum Command {
    /// Quit the application
    Quit,
    /// Open a URL in the current tab
    Open(String),
    /// Open a URL in a new tab
    TabOpen(String),
    /// Save page to file
    Write(String),
    /// List tabs
    Tabs,
    /// Show history
    History,
    /// Set a configuration option
    Set(String, String),
    /// Unknown command
    Unknown(String),
    /// Empty command
    Empty,
}

impl Command {
    /// Parse a command string
    pub fn parse(input: &str) -> Self {
        let input = input.trim();
        if input.is_empty() {
            return Command::Empty;
        }

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| s.trim().to_string());

        match cmd {
            "q" | "quit" | "exit" => Command::Quit,
            "o" | "open" | "e" | "edit" => {
                if let Some(url) = arg {
                    Command::Open(url)
                } else {
                    Command::Unknown("open requires a URL".to_string())
                }
            }
            "t" | "tabo" | "tabnew" | "tabopen" => {
                if let Some(url) = arg {
                    Command::TabOpen(url)
                } else {
                    Command::Unknown("tabopen requires a URL".to_string())
                }
            }
            "w" | "write" | "save" => {
                if let Some(path) = arg {
                    Command::Write(path)
                } else {
                    Command::Write("page.md".to_string())
                }
            }
            "tabs" | "buffers" | "ls" => Command::Tabs,
            "history" | "hist" => Command::History,
            "set" => {
                if let Some(setting) = arg {
                    let setting_parts: Vec<&str> = setting.splitn(2, '=').collect();
                    if setting_parts.len() == 2 {
                        Command::Set(
                            setting_parts[0].trim().to_string(),
                            setting_parts[1].trim().to_string(),
                        )
                    } else {
                        Command::Unknown(format!("set requires key=value format"))
                    }
                } else {
                    Command::Unknown("set requires a setting".to_string())
                }
            }
            _ => Command::Unknown(cmd.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quit() {
        assert!(matches!(Command::parse("q"), Command::Quit));
        assert!(matches!(Command::parse("quit"), Command::Quit));
    }

    #[test]
    fn test_parse_open() {
        match Command::parse("o example.com") {
            Command::Open(url) => assert_eq!(url, "example.com"),
            _ => panic!("Expected Open command"),
        }
    }

    #[test]
    fn test_parse_tabopen() {
        match Command::parse("t example.com") {
            Command::TabOpen(url) => assert_eq!(url, "example.com"),
            _ => panic!("Expected TabOpen command"),
        }
    }

    #[test]
    fn test_parse_set() {
        match Command::parse("set javascript=false") {
            Command::Set(key, value) => {
                assert_eq!(key, "javascript");
                assert_eq!(value, "false");
            }
            _ => panic!("Expected Set command"),
        }
    }
}
