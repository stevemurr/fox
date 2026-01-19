//! HTTP and headless browser fetching

use crate::accessibility::{ax_tree_to_markdown, fetch_ax_tree};
use crate::chrome::{BrowserHandle, ChromeConfig, ChromeManager, ExtractionMethod};
use crate::{extract, ExtractedContent, FetchConfig, FoxError, Page, Result};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use url::Url;

/// Fetcher for web pages
pub struct Fetcher {
    client: Client,
    chrome_manager: ChromeManager,
    browser: Option<Arc<Mutex<BrowserHandle>>>,
    config: FetchConfig,
    extraction_method: ExtractionMethod,
}

impl Fetcher {
    /// Create a new fetcher with default configuration
    pub async fn new() -> Result<Self> {
        Self::with_config(FetchConfig::default()).await
    }

    /// Create a new fetcher with custom configuration
    pub async fn with_config(config: FetchConfig) -> Result<Self> {
        Self::with_config_and_chrome(config, ChromeConfig::default()).await
    }

    /// Create a new fetcher with custom fetch and Chrome configuration
    pub async fn with_config_and_chrome(
        config: FetchConfig,
        chrome_config: ChromeConfig,
    ) -> Result<Self> {
        let client = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(Duration::from_secs(config.timeout_secs))
            .cookie_store(true)
            .build()?;

        let extraction_method = chrome_config.extraction_method;
        let mut chrome_manager = ChromeManager::with_config(chrome_config);

        let browser = if config.javascript {
            match chrome_manager.get_browser().await {
                Ok(b) => Some(b),
                Err(e) => {
                    warn!(
                        "Failed to initialize browser, falling back to HTTP-only: {}",
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            client,
            chrome_manager,
            browser,
            config,
            extraction_method,
        })
    }

    /// Fetch a page by URL
    pub async fn fetch(&self, url: &str) -> Result<Page> {
        let url = Url::parse(url)?;
        info!("Fetching: {}", url);

        // Determine if we should use accessibility tree extraction
        let use_ax_tree = self.config.javascript
            && self.browser.is_some()
            && self.extraction_method == ExtractionMethod::Accessibility;

        if use_ax_tree {
            // Use accessibility tree extraction (Chrome + AX tree)
            self.fetch_with_ax_tree(&url).await
        } else if self.config.javascript && self.browser.is_some() {
            // Use browser but with readability extraction
            let html = self.fetch_with_browser(&url).await?;
            let title = extract::extract_title(&html);
            let content = if self.config.extract_content {
                Some(extract::extract_content(&html, &url)?)
            } else {
                None
            };
            Ok(Page {
                url,
                title,
                html,
                content,
            })
        } else {
            // HTTP-only mode with readability extraction
            let html = self.fetch_with_http(&url).await?;
            let title = extract::extract_title(&html);
            let content = if self.config.extract_content {
                Some(extract::extract_content(&html, &url)?)
            } else {
                None
            };
            Ok(Page {
                url,
                title,
                html,
                content,
            })
        }
    }

    /// Fetch using HTTP only (no JavaScript)
    pub async fn fetch_with_http(&self, url: &Url) -> Result<String> {
        debug!("Fetching with HTTP: {}", url);
        let response = self.client.get(url.as_str()).send().await?;
        let html = response.text().await?;
        Ok(html)
    }

    /// Fetch using headless browser (with JavaScript)
    async fn fetch_with_browser(&self, url: &Url) -> Result<String> {
        debug!("Fetching with browser: {}", url);

        let browser_handle = self.browser.as_ref().unwrap();
        let handle = browser_handle.lock().await;

        let page = handle
            .browser
            .new_page(url.as_str())
            .await
            .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        // Wait for the page to load
        page.wait_for_navigation()
            .await
            .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        // Get the rendered HTML
        let html = page
            .content()
            .await
            .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        // Close the page
        let _ = page.close().await;

        Ok(html)
    }

    /// Fetch using headless browser with accessibility tree extraction
    async fn fetch_with_ax_tree(&self, url: &Url) -> Result<Page> {
        debug!("Fetching with accessibility tree: {}", url);

        let browser_handle = self.browser.as_ref().unwrap();
        let handle = browser_handle.lock().await;

        let page = handle
            .browser
            .new_page(url.as_str())
            .await
            .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        // Wait for the page to load
        page.wait_for_navigation()
            .await
            .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        // Small delay for dynamic content to settle
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Get the rendered HTML (for the Page struct)
        let html = page
            .content()
            .await
            .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        // Extract title from HTML
        let title = extract::extract_title(&html);

        // Fetch and convert the accessibility tree
        let content = if self.config.extract_content {
            debug!("Fetching accessibility tree...");
            match fetch_ax_tree(&page).await {
                Ok(tree) => {
                    let (markdown, links) = ax_tree_to_markdown(&tree);
                    debug!("AX tree converted: {} chars, {} links", markdown.len(), links.len());
                    Some(ExtractedContent {
                        text: markdown,
                        title: title.clone(),
                        links,
                    })
                }
                Err(e) => {
                    warn!("Failed to fetch AX tree, falling back to readability: {}", e);
                    // Fallback to readability extraction
                    Some(extract::extract_content(&html, url)?)
                }
            }
        } else {
            None
        };

        // Close the page
        let _ = page.close().await;

        Ok(Page {
            url: url.clone(),
            title,
            html,
            content,
        })
    }

    /// Fetch a page without JavaScript rendering
    pub async fn fetch_no_js(&self, url: &str) -> Result<Page> {
        let url = Url::parse(url)?;
        let html = self.fetch_with_http(&url).await?;
        let title = extract::extract_title(&html);
        let content = if self.config.extract_content {
            Some(extract::extract_content(&html, &url)?)
        } else {
            None
        };

        Ok(Page {
            url,
            title,
            html,
            content,
        })
    }

    /// Render HTML from a string (no fetching)
    pub fn render_html(&self, html: &str, base_url: Option<&str>) -> Result<Page> {
        let url = base_url
            .map(|u| Url::parse(u))
            .transpose()?
            .unwrap_or_else(|| Url::parse("about:blank").unwrap());

        let title = extract::extract_title(html);
        let content = if self.config.extract_content {
            Some(extract::extract_content(html, &url)?)
        } else {
            None
        };

        Ok(Page {
            url,
            title,
            html: html.to_string(),
            content,
        })
    }
}

impl Fetcher {
    /// Get a reference to the Chrome manager
    pub fn chrome_manager(&self) -> &ChromeManager {
        &self.chrome_manager
    }

    /// Check if JavaScript rendering is available
    pub fn has_javascript(&self) -> bool {
        self.browser.is_some()
    }

    /// Get the current extraction method
    pub fn extraction_method(&self) -> ExtractionMethod {
        self.extraction_method
    }

    /// Check if using accessibility tree extraction
    pub fn uses_accessibility_tree(&self) -> bool {
        self.browser.is_some() && self.extraction_method == ExtractionMethod::Accessibility
    }
}

impl Drop for Fetcher {
    fn drop(&mut self) {
        // Browser cleanup is handled by the async runtime
    }
}

/// Simple HTTP-only fetch function for quick use
pub async fn fetch_simple(url: &str) -> Result<Page> {
    let config = FetchConfig {
        javascript: false,
        ..Default::default()
    };
    let fetcher = Fetcher::with_config(config).await?;
    fetcher.fetch(url).await
}
