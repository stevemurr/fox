//! Fox Core Library
//!
//! Core functionality for the Fox terminal browser including:
//! - HTTP and headless browser fetching
//! - Readability content extraction
//! - HTML to Markdown conversion

pub mod extract;
pub mod fetch;
pub mod markdown;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum FoxError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),

    #[error("Browser error: {0}")]
    BrowserError(String),

    #[error("Content extraction failed: {0}")]
    ExtractionError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, FoxError>;

/// Represents a fetched web page
#[derive(Debug, Clone)]
pub struct Page {
    /// The URL of the page
    pub url: url::Url,
    /// The page title
    pub title: Option<String>,
    /// The raw HTML content
    pub html: String,
    /// Extracted main content (if extraction was performed)
    pub content: Option<ExtractedContent>,
}

/// Extracted content from a web page
#[derive(Debug, Clone)]
pub struct ExtractedContent {
    /// The main text content
    pub text: String,
    /// The title extracted from the content
    pub title: Option<String>,
    /// Links found in the content
    pub links: Vec<Link>,
}

/// A link found in the page
#[derive(Debug, Clone)]
pub struct Link {
    /// The display text
    pub text: String,
    /// The URL
    pub url: String,
    /// Position in the markdown content (character offset)
    pub position: usize,
}

impl Page {
    /// Convert the page content to markdown
    pub fn to_markdown(&self) -> String {
        if let Some(ref content) = self.content {
            content.text.clone()
        } else {
            markdown::html_to_markdown(&self.html)
        }
    }

    /// Convert the page content to plain text
    pub fn to_plain_text(&self) -> String {
        let md = self.to_markdown();
        markdown::markdown_to_plain(&md)
    }
}

/// Configuration for fetching pages
#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// Whether to use JavaScript rendering
    pub javascript: bool,
    /// User agent string
    pub user_agent: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Whether to extract content using readability
    pub extract_content: bool,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            javascript: true,
            user_agent: format!("Fox/{} (Terminal Browser)", env!("CARGO_PKG_VERSION")),
            timeout_secs: 30,
            extract_content: true,
        }
    }
}
