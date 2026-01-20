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
    /// Link hints for hint mode (multi-letter hints like qutebrowser)
    pub link_hints: Vec<(String, Link)>,
    /// Current hint input buffer for multi-letter hints
    pub hint_input: String,
    /// Search results positions
    pub search_results: Vec<usize>,
    /// Current search result index
    pub search_index: usize,
    /// Last search query
    pub last_search: String,
    /// URL suggestions for :o command (fuzzy filtered from history)
    pub url_suggestions: Vec<UrlSuggestion>,
    /// Currently selected suggestion index
    pub suggestion_index: usize,
}

/// A URL suggestion from history
#[derive(Clone, Debug)]
pub struct UrlSuggestion {
    pub url: String,
    pub title: Option<String>,
    pub score: i32,
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
            hint_input: String::new(),
            search_results: Vec::new(),
            search_index: 0,
            last_search: String::new(),
            url_suggestions: Vec::new(),
            suggestion_index: 0,
        })
    }

    /// Navigate to a URL in the current tab
    pub async fn navigate(&mut self, url: &str) -> Result<()> {
        self.navigate_internal(url, true).await
    }

    /// Navigate without adding to history (for back/forward)
    async fn navigate_without_history(&mut self, url: &str) -> Result<()> {
        self.navigate_internal(url, false).await
    }

    /// Internal navigation with optional history tracking
    async fn navigate_internal(&mut self, url: &str, add_to_history: bool) -> Result<()> {
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
                if add_to_history {
                    self.history.add(&url, page.title.as_deref());
                }
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
                    self.navigate_without_history(&url).await?;
                }
            }
            KeyCode::Char('L') => {
                if let Some(url) = self.history.forward() {
                    self.navigate_without_history(&url).await?;
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
                self.url_suggestions.clear();
                self.suggestion_index = 0;
            }
            KeyCode::Enter => {
                // If suggestion is selected, use it
                if !self.url_suggestions.is_empty() && self.suggestion_index < self.url_suggestions.len() {
                    let suggestion = &self.url_suggestions[self.suggestion_index];
                    // Replace the URL part of the command with the suggestion
                    if let Some(prefix) = self.get_url_command_prefix() {
                        self.input = format!("{} {}", prefix, suggestion.url);
                    }
                }

                let command = self.input.clone();
                self.vim.mode = VimMode::Normal;
                self.input.clear();
                self.url_suggestions.clear();
                self.suggestion_index = 0;
                return self.execute_command(&command).await;
            }
            KeyCode::Tab => {
                // Auto-complete with selected suggestion
                if !self.url_suggestions.is_empty() && self.suggestion_index < self.url_suggestions.len() {
                    let suggestion = &self.url_suggestions[self.suggestion_index];
                    if let Some(prefix) = self.get_url_command_prefix() {
                        self.input = format!("{} {}", prefix, suggestion.url);
                        self.url_suggestions.clear();
                        self.suggestion_index = 0;
                    }
                }
            }
            KeyCode::Up => {
                if !self.url_suggestions.is_empty() {
                    if self.suggestion_index > 0 {
                        self.suggestion_index -= 1;
                    } else {
                        self.suggestion_index = self.url_suggestions.len() - 1;
                    }
                }
            }
            KeyCode::Down => {
                if !self.url_suggestions.is_empty() {
                    self.suggestion_index = (self.suggestion_index + 1) % self.url_suggestions.len();
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.update_url_suggestions();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                self.update_url_suggestions();
            }
            _ => {}
        }
        Ok(false)
    }

    /// Get the command prefix if input is an open/tabopen command
    fn get_url_command_prefix(&self) -> Option<&str> {
        let input = self.input.trim();
        if input.starts_with("o ") || input.starts_with("open ") {
            Some(input.split_whitespace().next().unwrap_or("o"))
        } else if input.starts_with("t ") || input.starts_with("tabo ") || input.starts_with("tabnew ") || input.starts_with("tabopen ") {
            Some(input.split_whitespace().next().unwrap_or("t"))
        } else {
            None
        }
    }

    /// Update URL suggestions based on current input
    fn update_url_suggestions(&mut self) {
        self.url_suggestions.clear();
        self.suggestion_index = 0;

        // Only show suggestions for open/tabopen commands
        let input = self.input.trim();
        let query = if input.starts_with("o ") {
            input.strip_prefix("o ").unwrap_or("")
        } else if input.starts_with("open ") {
            input.strip_prefix("open ").unwrap_or("")
        } else if input.starts_with("t ") {
            input.strip_prefix("t ").unwrap_or("")
        } else if input.starts_with("tabo ") {
            input.strip_prefix("tabo ").unwrap_or("")
        } else if input.starts_with("tabnew ") {
            input.strip_prefix("tabnew ").unwrap_or("")
        } else if input.starts_with("tabopen ") {
            input.strip_prefix("tabopen ").unwrap_or("")
        } else {
            return; // Not an open command
        };

        // Get recent history and fuzzy filter
        let max_suggestions = 10;

        if query.is_empty() {
            // Show recent history when no query
            self.url_suggestions = self.history
                .recent(max_suggestions)
                .into_iter()
                .map(|e| UrlSuggestion {
                    url: e.url.clone(),
                    title: e.title.clone(),
                    score: 0,
                })
                .collect();
        } else {
            // Fuzzy filter history
            let mut matches: Vec<UrlSuggestion> = self.history
                .recent(100) // Search in more entries
                .into_iter()
                .filter_map(|e| {
                    let score = fuzzy_match(&e.url, query)
                        .or_else(|| e.title.as_ref().and_then(|t| fuzzy_match(t, query)));
                    score.map(|s| UrlSuggestion {
                        url: e.url.clone(),
                        title: e.title.clone(),
                        score: s,
                    })
                })
                .collect();

            // Sort by score (higher is better)
            matches.sort_by(|a, b| b.score.cmp(&a.score));

            // Take top matches
            self.url_suggestions = matches.into_iter().take(max_suggestions).collect();
        }
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
                self.hint_input.clear();
            }
            KeyCode::Backspace => {
                self.hint_input.pop();
            }
            KeyCode::Char(c) => {
                self.hint_input.push(c);

                // Check for exact match
                if let Some((_, link)) = self.link_hints.iter().find(|(h, _)| h == &self.hint_input) {
                    let url = link.url.clone();
                    self.vim.mode = VimMode::Normal;
                    self.link_hints.clear();
                    self.hint_input.clear();
                    self.navigate(&url).await?;
                } else {
                    // Check if any hints start with current input (still valid prefix)
                    let has_match = self.link_hints.iter().any(|(h, _)| h.starts_with(&self.hint_input));
                    if !has_match {
                        // Invalid input, reset
                        self.hint_input.clear();
                    }
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
            if let Some(content) = tab.content() {
                let scroll_offset = tab.scroll_offset();
                let viewport_height = tab.viewport_height;
                let visible_start = scroll_offset;
                let visible_end = scroll_offset + viewport_height;

                // Calculate line number for each link and filter to visible ones
                let mut visible_links: Vec<(usize, &Link)> = links
                    .iter()
                    .filter_map(|link| {
                        // Find which line contains this link
                        let line_num = content[..link.position.min(content.len())]
                            .lines()
                            .count()
                            .saturating_sub(1);
                        // Only include if in visible range
                        if line_num >= visible_start && line_num < visible_end {
                            Some((line_num, link))
                        } else {
                            None
                        }
                    })
                    .collect();

                // Sort by line number (top to bottom)
                visible_links.sort_by_key(|(line, _)| *line);

                // Generate multi-letter hints for all visible links
                let hints = generate_hints(visible_links.len());
                self.link_hints = visible_links
                    .into_iter()
                    .zip(hints.into_iter())
                    .map(|((_, link), hint)| (hint, link.clone()))
                    .collect();

                self.hint_input.clear();
            }
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

/// Generate multi-letter hints for a given count of links
/// Uses home-row keys for easier typing, similar to qutebrowser
fn generate_hints(count: usize) -> Vec<String> {
    if count == 0 {
        return vec![];
    }

    // Use home row keys first (easier to type), then other keys
    const HINT_CHARS: &[char] = &[
        'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', // Home row first
        'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', // Top row
        'z', 'x', 'c', 'v', 'b', 'n', 'm', // Bottom row
    ];

    let base = HINT_CHARS.len();

    // Calculate minimum hint length needed
    let hint_len = if count <= base {
        1
    } else if count <= base * base {
        2
    } else {
        3 // Should be enough for most pages (27^3 = ~19000 links)
    };

    let mut hints = Vec::with_capacity(count);

    for i in 0..count {
        let hint = match hint_len {
            1 => HINT_CHARS[i].to_string(),
            2 => {
                let first = HINT_CHARS[i / base];
                let second = HINT_CHARS[i % base];
                format!("{}{}", first, second)
            }
            _ => {
                let first = HINT_CHARS[i / (base * base)];
                let second = HINT_CHARS[(i / base) % base];
                let third = HINT_CHARS[i % base];
                format!("{}{}{}", first, second, third)
            }
        };
        hints.push(hint);
    }

    hints
}

/// Simple fuzzy match function that returns a score if pattern matches text
/// Higher score = better match
/// Returns None if no match
fn fuzzy_match(text: &str, pattern: &str) -> Option<i32> {
    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();

    // Quick check: all pattern chars must exist in text
    let mut text_iter = text_lower.chars().peekable();
    for p_char in pattern_lower.chars() {
        loop {
            match text_iter.next() {
                Some(t_char) if t_char == p_char => break,
                Some(_) => continue,
                None => return None,
            }
        }
    }

    // Calculate score based on matching quality
    let mut score: i32 = 0;

    // Bonus for exact substring match
    if text_lower.contains(&pattern_lower) {
        score += 100;

        // Extra bonus for prefix match
        if text_lower.starts_with(&pattern_lower) {
            score += 50;
        }
    }

    // Bonus for shorter URLs (less noise)
    score += 200 - text.len().min(200) as i32;

    // Bonus for pattern characters appearing close together in text
    let mut last_match_pos: Option<usize> = None;
    let mut gap_penalty = 0;
    let mut text_iter = text_lower.char_indices();
    for p_char in pattern_lower.chars() {
        while let Some((pos, t_char)) = text_iter.next() {
            if t_char == p_char {
                if let Some(last_pos) = last_match_pos {
                    // Penalize large gaps between matches
                    let gap = pos - last_pos - 1;
                    gap_penalty += gap.min(10) as i32;
                }
                last_match_pos = Some(pos);
                break;
            }
        }
    }
    score -= gap_penalty;

    Some(score)
}
