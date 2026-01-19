//! Application state management

use crate::config::Config;
use crate::history::History;
use crate::tabs::TabManager;
use crate::vim::{Command, VimMode, VimState};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fox_core::fetch::Fetcher;
use fox_core::{FetchConfig, Link};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

/// Main application state
pub struct App {
    /// Tab manager
    pub tabs: TabManager,
    /// Navigation history
    pub history: History,
    /// Vim mode state
    pub vim: VimState,
    /// Current input (for command/search mode)
    pub input: String,
    /// Status message
    pub status: Option<String>,
    /// Whether loading is in progress
    pub loading: bool,
    /// Configuration
    pub config: Config,
    /// HTTP/Browser fetcher
    fetcher: Arc<Mutex<Fetcher>>,
    /// Pending key for multi-key commands
    pending_key: Option<char>,
    /// Link hints for hint mode
    pub link_hints: Vec<(char, Link)>,
    /// Search results positions
    pub search_results: Vec<usize>,
    /// Current search result index
    pub search_index: usize,
    /// Last search query
    pub last_search: String,
}

impl App {
    /// Create a new application instance
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;

        let fetch_config = FetchConfig {
            javascript: config.javascript,
            ..Default::default()
        };

        let chrome_config = config.to_chrome_config();
        let fetcher = Fetcher::with_config_and_chrome(fetch_config, chrome_config).await?;

        Ok(Self {
            tabs: TabManager::new(),
            history: History::load()?,
            vim: VimState::new(),
            input: String::new(),
            status: Some("Welcome to Fox! Press : to enter commands, or :o <url> to navigate".to_string()),
            loading: false,
            config,
            fetcher: Arc::new(Mutex::new(fetcher)),
            pending_key: None,
            link_hints: Vec::new(),
            search_results: Vec::new(),
            search_index: 0,
            last_search: String::new(),
        })
    }

    /// Navigate to a URL in the current tab
    pub async fn navigate(&mut self, url: &str) -> Result<()> {
        self.loading = true;
        self.status = Some(format!("Loading {}...", url));

        // Normalize URL
        let url = if !url.contains("://") {
            format!("https://{}", url)
        } else {
            url.to_string()
        };

        let fetcher = self.fetcher.lock().await;
        match fetcher.fetch(&url).await {
            Ok(page) => {
                self.history.add(&url, page.title.as_deref());
                self.tabs.current_mut().load_page(page);
                self.status = None;
            }
            Err(e) => {
                self.status = Some(format!("Error: {}", e));
            }
        }

        self.loading = false;
        Ok(())
    }

    /// Navigate to a URL in a new tab
    pub async fn navigate_new_tab(&mut self, url: &str) -> Result<()> {
        self.tabs.new_tab();
        self.navigate(url).await
    }

    /// Handle a key event
    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        match self.vim.mode {
            VimMode::Normal => self.handle_normal_key(key).await,
            VimMode::Command => self.handle_command_key(key).await,
            VimMode::Search => self.handle_search_key(key).await,
            VimMode::Insert => self.handle_insert_key(key).await,
            VimMode::Hint => self.handle_hint_key(key).await,
        }
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) -> Result<bool> {
        let tab = self.tabs.current_mut();

        // Handle multi-key commands
        if let Some(pending) = self.pending_key.take() {
            match (pending, key.code) {
                ('g', KeyCode::Char('g')) => tab.scroll_to_top(),
                ('g', KeyCode::Char('t')) => self.tabs.next_tab(),
                ('g', KeyCode::Char('T')) => self.tabs.prev_tab(),
                ('g', KeyCode::Char(n)) if n.is_ascii_digit() => {
                    let idx = n.to_digit(10).unwrap_or(1) as usize;
                    self.tabs.go_to_tab(idx.saturating_sub(1));
                }
                _ => {}
            }
            return Ok(false);
        }

        match key.code {
            // Scrolling
            KeyCode::Char('j') | KeyCode::Down => tab.scroll_down(1),
            KeyCode::Char('k') | KeyCode::Up => tab.scroll_up(1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                tab.scroll_down(tab.viewport_height / 2)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                tab.scroll_up(tab.viewport_height / 2)
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                tab.scroll_down(tab.viewport_height)
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                tab.scroll_up(tab.viewport_height)
            }
            KeyCode::Char('G') => tab.scroll_to_bottom(),
            KeyCode::Char('g') => {
                self.pending_key = Some('g');
            }

            // Link navigation
            KeyCode::Char('h') => tab.prev_link(),
            KeyCode::Char('l') => tab.next_link(),
            KeyCode::Char('f') => {
                self.enter_hint_mode();
            }
            KeyCode::Enter => {
                if let Some(link) = tab.selected_link() {
                    let url = link.url.clone();
                    self.navigate(&url).await?;
                }
            }

            // History
            KeyCode::Char('H') => {
                if let Some(url) = self.history.back() {
                    self.navigate(&url).await?;
                }
            }
            KeyCode::Char('L') => {
                if let Some(url) = self.history.forward() {
                    self.navigate(&url).await?;
                }
            }

            // Tab management
            KeyCode::Char('d') => {
                if self.tabs.len() > 1 {
                    self.tabs.close_current();
                } else {
                    return Ok(true); // Quit if last tab
                }
            }

            // Mode switching
            KeyCode::Char(':') => {
                self.vim.mode = VimMode::Command;
                self.input.clear();
            }
            KeyCode::Char('/') => {
                self.vim.mode = VimMode::Search;
                self.input.clear();
            }
            KeyCode::Char('i') => {
                self.vim.mode = VimMode::Insert;
            }

            // Search navigation
            KeyCode::Char('n') => {
                self.next_search_result();
            }
            KeyCode::Char('N') => {
                self.prev_search_result();
            }

            // Clipboard
            KeyCode::Char('y') => {
                if let Some(url) = tab.url() {
                    self.yank_to_clipboard(&url);
                }
            }
            KeyCode::Char('p') => {
                if let Some(url) = self.paste_from_clipboard() {
                    self.navigate(&url).await?;
                }
            }

            _ => {}
        }

        Ok(false)
    }

    async fn handle_command_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.vim.mode = VimMode::Normal;
                self.input.clear();
            }
            KeyCode::Enter => {
                let command = self.input.clone();
                self.vim.mode = VimMode::Normal;
                self.input.clear();
                return self.execute_command(&command).await;
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_search_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.vim.mode = VimMode::Normal;
                self.input.clear();
            }
            KeyCode::Enter => {
                let query = self.input.clone();
                self.vim.mode = VimMode::Normal;
                self.search(&query);
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_insert_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.vim.mode = VimMode::Normal;
            }
            KeyCode::Tab => {
                self.tabs.current_mut().next_form_field();
            }
            KeyCode::BackTab => {
                self.tabs.current_mut().prev_form_field();
            }
            _ => {
                // In a full implementation, this would handle form input
            }
        }
        Ok(false)
    }

    async fn handle_hint_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.vim.mode = VimMode::Normal;
                self.link_hints.clear();
            }
            KeyCode::Char(c) => {
                // Find the link matching this hint
                if let Some((_, link)) = self.link_hints.iter().find(|(h, _)| *h == c) {
                    let url = link.url.clone();
                    self.vim.mode = VimMode::Normal;
                    self.link_hints.clear();
                    self.navigate(&url).await?;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn execute_command(&mut self, input: &str) -> Result<bool> {
        let cmd = Command::parse(input);
        debug!("Executing command: {:?}", cmd);

        match cmd {
            Command::Quit => return Ok(true),
            Command::Open(url) => {
                self.navigate(&url).await?;
            }
            Command::TabOpen(url) => {
                self.navigate_new_tab(&url).await?;
            }
            Command::Write(path) => {
                self.save_page(&path)?;
            }
            Command::Tabs => {
                self.status = Some(self.tabs.list_tabs());
            }
            Command::History => {
                self.status = Some(self.history.list());
            }
            Command::Set(key, value) => {
                self.config.set(&key, &value)?;
            }
            Command::Unknown(cmd) => {
                self.status = Some(format!("Unknown command: {}", cmd));
            }
            Command::Empty => {}
        }

        Ok(false)
    }

    fn enter_hint_mode(&mut self) {
        let tab = self.tabs.current();
        if let Some(links) = tab.links() {
            let hints = "asdfghjklqwertyuiopzxcvbnm";
            self.link_hints = links
                .iter()
                .take(hints.len())
                .zip(hints.chars())
                .map(|(link, hint)| (hint, link.clone()))
                .collect();
            self.vim.mode = VimMode::Hint;
        }
    }

    fn search(&mut self, query: &str) {
        self.last_search = query.to_string();
        self.search_results.clear();
        self.search_index = 0;

        let tab = self.tabs.current();
        if let Some(content) = tab.content() {
            let query_lower = query.to_lowercase();
            let content_lower = content.to_lowercase();

            let mut pos = 0;
            while let Some(idx) = content_lower[pos..].find(&query_lower) {
                self.search_results.push(pos + idx);
                pos += idx + 1;
            }

            if !self.search_results.is_empty() {
                self.status = Some(format!(
                    "Found {} matches",
                    self.search_results.len()
                ));
                self.jump_to_search_result();
            } else {
                self.status = Some(format!("Pattern not found: {}", query));
            }
        }
    }

    fn next_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.search_index = (self.search_index + 1) % self.search_results.len();
            self.jump_to_search_result();
        }
    }

    fn prev_search_result(&mut self) {
        if !self.search_results.is_empty() {
            self.search_index = if self.search_index == 0 {
                self.search_results.len() - 1
            } else {
                self.search_index - 1
            };
            self.jump_to_search_result();
        }
    }

    fn jump_to_search_result(&mut self) {
        if let Some(&pos) = self.search_results.get(self.search_index) {
            // Approximate line number from character position
            let tab = self.tabs.current_mut();
            if let Some(content) = tab.content() {
                let line = content[..pos].lines().count();
                tab.scroll_to_line(line);
            }
        }
    }

    fn save_page(&self, path: &str) -> Result<()> {
        let tab = self.tabs.current();
        if let Some(content) = tab.content() {
            std::fs::write(path, content)?;
        }
        Ok(())
    }

    fn yank_to_clipboard(&mut self, text: &str) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                if clipboard.set_text(text).is_ok() {
                    self.status = Some(format!("Yanked: {}", text));
                }
            }
            Err(_) => {
                self.status = Some("Clipboard not available".to_string());
            }
        }
    }

    fn paste_from_clipboard(&mut self) -> Option<String> {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut c| c.get_text().ok())
    }

    /// Process any pending async operations
    pub async fn tick(&mut self) -> Result<()> {
        // Future: handle pending fetches, websockets, etc.
        Ok(())
    }

    /// Update viewport size
    pub fn set_viewport_size(&mut self, width: u16, height: u16) {
        self.tabs.current_mut().set_viewport_size(width, height);
    }
}
