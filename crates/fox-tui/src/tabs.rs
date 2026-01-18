//! Tab/buffer management

use fox_core::{Link, Page};
use textwrap;

/// A single browser tab
#[derive(Debug)]
pub struct Tab {
    /// The loaded page (if any)
    page: Option<Page>,
    /// Rendered markdown content
    rendered_lines: Vec<String>,
    /// Current scroll position (line number)
    scroll_offset: usize,
    /// Viewport height in lines
    pub viewport_height: usize,
    /// Viewport width in columns
    pub viewport_width: usize,
    /// Currently selected link index
    selected_link: Option<usize>,
    /// Current form field index
    form_field_index: usize,
}

impl Default for Tab {
    fn default() -> Self {
        Self {
            page: None,
            rendered_lines: vec![
                "".to_string(),
                "  Welcome to Fox - Terminal Web Browser".to_string(),
                "".to_string(),
                "  Commands:".to_string(),
                "    :o <url>  - Open a URL".to_string(),
                "    :t <url>  - Open in new tab".to_string(),
                "    :q        - Quit".to_string(),
                "".to_string(),
                "  Navigation:".to_string(),
                "    j/k       - Scroll down/up".to_string(),
                "    gg/G      - Top/bottom".to_string(),
                "    Ctrl-d/u  - Half page down/up".to_string(),
                "    f         - Follow link (hint mode)".to_string(),
                "    H/L       - History back/forward".to_string(),
                "".to_string(),
                "  Tabs:".to_string(),
                "    gt/gT     - Next/previous tab".to_string(),
                "    d         - Close tab".to_string(),
                "".to_string(),
            ],
            scroll_offset: 0,
            viewport_height: 24,
            viewport_width: 80,
            selected_link: None,
            form_field_index: 0,
        }
    }
}

impl Tab {
    /// Create a new empty tab
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a page into the tab
    pub fn load_page(&mut self, page: Page) {
        let content = page.to_markdown();
        self.rendered_lines = self.wrap_content(&content);
        self.page = Some(page);
        self.scroll_offset = 0;
        self.selected_link = if self.links().map(|l| l.is_empty()).unwrap_or(true) {
            None
        } else {
            Some(0)
        };
    }

    fn wrap_content(&self, content: &str) -> Vec<String> {
        let width = self.viewport_width.saturating_sub(2).max(20);
        content
            .lines()
            .flat_map(|line| {
                if line.trim().is_empty() {
                    vec![String::new()]
                } else {
                    textwrap::wrap(line, width)
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                }
            })
            .collect()
    }

    /// Get the page URL
    pub fn url(&self) -> Option<String> {
        self.page.as_ref().map(|p| p.url.to_string())
    }

    /// Get the page title
    pub fn title(&self) -> Option<&str> {
        self.page
            .as_ref()
            .and_then(|p| p.title.as_deref())
    }

    /// Get the markdown content
    pub fn content(&self) -> Option<&str> {
        self.page
            .as_ref()
            .and_then(|p| p.content.as_ref())
            .map(|c| c.text.as_str())
    }

    /// Get the visible lines
    pub fn visible_lines(&self) -> &[String] {
        let end = (self.scroll_offset + self.viewport_height).min(self.rendered_lines.len());
        &self.rendered_lines[self.scroll_offset..end]
    }

    /// Get total number of lines
    pub fn total_lines(&self) -> usize {
        self.rendered_lines.len()
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Scroll down by n lines
    pub fn scroll_down(&mut self, n: usize) {
        let max = self.rendered_lines.len().saturating_sub(self.viewport_height);
        self.scroll_offset = (self.scroll_offset + n).min(max);
    }

    /// Scroll up by n lines
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.rendered_lines.len().saturating_sub(self.viewport_height);
    }

    /// Scroll to a specific line
    pub fn scroll_to_line(&mut self, line: usize) {
        self.scroll_offset = line.saturating_sub(self.viewport_height / 2);
        let max = self.rendered_lines.len().saturating_sub(self.viewport_height);
        self.scroll_offset = self.scroll_offset.min(max);
    }

    /// Set viewport size
    pub fn set_viewport_size(&mut self, width: u16, height: u16) {
        self.viewport_width = width as usize;
        self.viewport_height = height.saturating_sub(4) as usize; // Leave room for status bars

        // Re-wrap content if we have a page
        if let Some(ref page) = self.page {
            let content = page.to_markdown();
            self.rendered_lines = self.wrap_content(&content);
        }
    }

    /// Get links in the page
    pub fn links(&self) -> Option<&[Link]> {
        self.page
            .as_ref()
            .and_then(|p| p.content.as_ref())
            .map(|c| c.links.as_slice())
    }

    /// Get the currently selected link
    pub fn selected_link(&self) -> Option<&Link> {
        self.selected_link
            .and_then(|idx| self.links().and_then(|links| links.get(idx)))
    }

    /// Get selected link index
    pub fn selected_link_index(&self) -> Option<usize> {
        self.selected_link
    }

    /// Move to next link
    pub fn next_link(&mut self) {
        if let Some(links) = self.links() {
            if !links.is_empty() {
                self.selected_link = Some(
                    self.selected_link
                        .map(|i| (i + 1) % links.len())
                        .unwrap_or(0),
                );
            }
        }
    }

    /// Move to previous link
    pub fn prev_link(&mut self) {
        if let Some(links) = self.links() {
            if !links.is_empty() {
                self.selected_link = Some(
                    self.selected_link
                        .map(|i| if i == 0 { links.len() - 1 } else { i - 1 })
                        .unwrap_or(0),
                );
            }
        }
    }

    /// Move to next form field
    pub fn next_form_field(&mut self) {
        self.form_field_index += 1;
    }

    /// Move to previous form field
    pub fn prev_form_field(&mut self) {
        self.form_field_index = self.form_field_index.saturating_sub(1);
    }
}

/// Manages multiple tabs
#[derive(Debug)]
pub struct TabManager {
    tabs: Vec<Tab>,
    current: usize,
}

impl Default for TabManager {
    fn default() -> Self {
        Self {
            tabs: vec![Tab::new()],
            current: 0,
        }
    }
}

impl TabManager {
    /// Create a new tab manager with one empty tab
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current tab
    pub fn current(&self) -> &Tab {
        &self.tabs[self.current]
    }

    /// Get the current tab mutably
    pub fn current_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.current]
    }

    /// Get current tab index
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Get all tabs
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Get number of tabs
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Check if there are no tabs
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Create a new tab
    pub fn new_tab(&mut self) {
        self.tabs.push(Tab::new());
        self.current = self.tabs.len() - 1;
    }

    /// Close the current tab
    pub fn close_current(&mut self) {
        if self.tabs.len() > 1 {
            self.tabs.remove(self.current);
            if self.current >= self.tabs.len() {
                self.current = self.tabs.len() - 1;
            }
        }
    }

    /// Switch to next tab
    pub fn next_tab(&mut self) {
        self.current = (self.current + 1) % self.tabs.len();
    }

    /// Switch to previous tab
    pub fn prev_tab(&mut self) {
        self.current = if self.current == 0 {
            self.tabs.len() - 1
        } else {
            self.current - 1
        };
    }

    /// Go to a specific tab by index
    pub fn go_to_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.current = index;
        }
    }

    /// List tabs as a string
    pub fn list_tabs(&self) -> String {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let marker = if i == self.current { ">" } else { " " };
                let title = tab.title().unwrap_or("New Tab");
                format!("{} {}: {}", marker, i + 1, title)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_scroll() {
        let mut tab = Tab::new();
        tab.viewport_height = 10;
        tab.rendered_lines = (0..50).map(|i| format!("Line {}", i)).collect();

        assert_eq!(tab.scroll_offset, 0);

        tab.scroll_down(5);
        assert_eq!(tab.scroll_offset, 5);

        tab.scroll_up(3);
        assert_eq!(tab.scroll_offset, 2);

        tab.scroll_to_bottom();
        assert_eq!(tab.scroll_offset, 40);

        tab.scroll_to_top();
        assert_eq!(tab.scroll_offset, 0);
    }

    #[test]
    fn test_tab_manager() {
        let mut manager = TabManager::new();
        assert_eq!(manager.len(), 1);

        manager.new_tab();
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.current_index(), 1);

        manager.prev_tab();
        assert_eq!(manager.current_index(), 0);

        manager.next_tab();
        assert_eq!(manager.current_index(), 1);

        manager.close_current();
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.current_index(), 0);
    }
}
