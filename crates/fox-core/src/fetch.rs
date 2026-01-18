//! HTTP and headless browser fetching

use crate::{extract, FetchConfig, FoxError, Page, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use url::Url;

/// Fetcher for web pages
pub struct Fetcher {
    client: Client,
    browser: Option<Arc<Mutex<BrowserHandle>>>,
    config: FetchConfig,
}

struct BrowserHandle {
    browser: Browser,
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
}

impl Fetcher {
    /// Create a new fetcher with default configuration
    pub async fn new() -> Result<Self> {
        Self::with_config(FetchConfig::default()).await
    }

    /// Create a new fetcher with custom configuration
    pub async fn with_config(config: FetchConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(Duration::from_secs(config.timeout_secs))
            .cookie_store(true)
            .build()?;

        let browser = if config.javascript {
            match Self::init_browser().await {
                Ok(b) => Some(Arc::new(Mutex::new(b))),
                Err(e) => {
                    warn!("Failed to initialize browser, falling back to HTTP-only: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            client,
            browser,
            config,
        })
    }

    async fn init_browser() -> Result<BrowserHandle> {
        let (browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .arg("--disable-gpu")
                .arg("--no-sandbox")
                .arg("--disable-dev-shm-usage")
                .build()
                .map_err(|e| FoxError::BrowserError(e.to_string()))?,
        )
        .await
        .map_err(|e| FoxError::BrowserError(e.to_string()))?;

        let handle = tokio::spawn(async move {
            while let Some(_) = handler.next().await {}
        });

        Ok(BrowserHandle { browser, handle })
    }

    /// Fetch a page by URL
    pub async fn fetch(&self, url: &str) -> Result<Page> {
        let url = Url::parse(url)?;
        info!("Fetching: {}", url);

        let html = if self.config.javascript && self.browser.is_some() {
            self.fetch_with_browser(&url).await?
        } else {
            self.fetch_with_http(&url).await?
        };

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
