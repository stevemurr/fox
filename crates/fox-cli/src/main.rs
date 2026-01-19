//! Fox CLI - Terminal Web Browser
//!
//! A hyper-modern, API-first terminal web browser with vim-mode navigation.

use anyhow::Result;
use clap::{Parser, Subcommand};
use fox_core::{fetch::Fetcher, FetchConfig, ChromeConfig, ChromeManager};
use std::io::{self, Read};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "fox")]
#[command(author, version, about = "A hyper-modern terminal web browser", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// URL to open (shortcut for `fox browse <url>`)
    url: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch a URL and output content to stdout
    Fetch {
        /// URL to fetch
        url: String,

        /// Output format: markdown, plain, json
        #[arg(short, long, default_value = "markdown")]
        format: OutputFormat,

        /// Skip JavaScript rendering (HTTP only)
        #[arg(long)]
        no_js: bool,

        /// Extraction method: accessibility (default) or readability
        #[arg(short, long, default_value = "accessibility")]
        extraction: String,
    },

    /// Open the interactive browser
    Browse {
        /// URL to open (optional)
        url: Option<String>,
    },

    /// Render HTML from stdin to markdown
    Render {
        /// Base URL for resolving relative links
        #[arg(short, long)]
        base_url: Option<String>,

        /// Output format: markdown, plain, json
        #[arg(short, long, default_value = "markdown")]
        format: OutputFormat,
    },

    /// Debug: dump accessibility tree for a URL
    #[command(name = "debug-ax")]
    DebugAx {
        /// URL to fetch
        url: String,

        /// Show full tree (include all nodes)
        #[arg(long)]
        full: bool,

        /// Also show markdown conversion
        #[arg(long, short)]
        markdown: bool,
    },
}

#[derive(Clone, Debug, Default)]
enum OutputFormat {
    #[default]
    Markdown,
    Plain,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "markdown" | "md" => Ok(OutputFormat::Markdown),
            "plain" | "text" | "txt" => Ok(OutputFormat::Plain),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    if cli.verbose {
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_subscriber::EnvFilter::new("debug"))
            .init();
    }

    match cli.command {
        Some(Commands::Fetch { url, format, no_js, extraction }) => {
            run_fetch(&url, format, no_js, &extraction).await?;
        }
        Some(Commands::Browse { url }) => {
            run_browse(url).await?;
        }
        Some(Commands::Render { base_url, format }) => {
            run_render(base_url, format).await?;
        }
        Some(Commands::DebugAx { url, full, markdown }) => {
            run_debug_ax(&url, full, markdown).await?;
        }
        None => {
            // If URL provided without subcommand, open browser
            if let Some(url) = cli.url {
                run_browse(Some(url)).await?;
            } else {
                // Open browser with blank page
                run_browse(None).await?;
            }
        }
    }

    Ok(())
}

async fn run_fetch(url: &str, format: OutputFormat, no_js: bool, extraction: &str) -> Result<()> {
    use fox_core::ExtractionMethod;

    let config = FetchConfig {
        javascript: !no_js,
        ..Default::default()
    };

    let extraction_method = extraction.parse::<ExtractionMethod>()
        .unwrap_or(ExtractionMethod::Accessibility);

    let chrome_config = ChromeConfig {
        extraction_method,
        ..Default::default()
    };

    let fetcher = Fetcher::with_config_and_chrome(config, chrome_config).await?;
    let page = if no_js {
        fetcher.fetch_no_js(url).await?
    } else {
        fetcher.fetch(url).await?
    };

    match format {
        OutputFormat::Markdown => {
            println!("{}", page.to_markdown());
        }
        OutputFormat::Plain => {
            println!("{}", page.to_plain_text());
        }
        OutputFormat::Json => {
            let json = serde_json::json!({
                "url": page.url.to_string(),
                "title": page.title,
                "content": page.to_markdown(),
                "links": page.content.as_ref().map(|c| c.links.iter().map(|l| {
                    serde_json::json!({
                        "text": l.text,
                        "url": l.url
                    })
                }).collect::<Vec<_>>()).unwrap_or_default()
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
    }

    Ok(())
}

async fn run_browse(url: Option<String>) -> Result<()> {
    fox_tui::run(url).await
}

async fn run_debug_ax(url: &str, full: bool, show_markdown: bool) -> Result<()> {
    use fox_core::accessibility::{fetch_ax_tree, ax_tree_to_markdown};

    // Initialize Chrome manager and get browser
    let mut chrome_manager = ChromeManager::with_config(ChromeConfig::default());
    let browser = chrome_manager.get_browser().await?;
    let browser_guard = browser.lock().await;

    // Create a new page and navigate to URL
    let url_parsed = if !url.contains("://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    };

    println!("Navigating to {}...", url_parsed);
    let page = browser_guard
        .browser
        .new_page(&url_parsed)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create page: {}", e))?;

    page.wait_for_navigation()
        .await
        .map_err(|e| anyhow::anyhow!("Navigation failed: {}", e))?;

    // Wait a bit for the page to fully render
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Fetch the accessibility tree
    println!("Fetching accessibility tree...\n");
    let tree = fetch_ax_tree(&page).await?;

    // Print the tree
    println!("Accessibility Tree ({} nodes):", tree.nodes.len());
    println!("=====================================\n");

    tree.walk(|node, depth| {
        // Skip empty generic nodes unless --full
        if !full && node.should_skip() && !node.has_content() {
            return;
        }

        let indent = "  ".repeat(depth);
        let name_display = node
            .name
            .as_ref()
            .map(|n| {
                let truncated: String = n.chars().take(60).collect();
                if n.len() > 60 {
                    format!("\"{}...\"", truncated)
                } else {
                    format!("\"{}\"", truncated)
                }
            })
            .unwrap_or_default();

        let level_display = node
            .level
            .map(|l| format!(" (level {})", l))
            .unwrap_or_default();

        let url_display = node
            .url
            .as_ref()
            .map(|u| format!(" -> {}", u))
            .unwrap_or_default();

        println!(
            "{}{}{}{} {}",
            indent, node.role, level_display, url_display, name_display
        );
    });

    // Show markdown conversion if requested
    if show_markdown {
        println!("\n=====================================");
        println!("Markdown Conversion:");
        println!("=====================================\n");

        let (markdown, links) = ax_tree_to_markdown(&tree);
        println!("{}", markdown);

        if !links.is_empty() {
            println!("\n--- Links found: {} ---", links.len());
            for (i, link) in links.iter().enumerate() {
                println!("  [{}] {} -> {}", i + 1, link.text, link.url);
            }
        }
    }

    // Close the page
    let _ = page.close().await;

    Ok(())
}

async fn run_render(base_url: Option<String>, format: OutputFormat) -> Result<()> {
    let mut html = String::new();
    io::stdin().read_to_string(&mut html)?;

    let config = FetchConfig {
        javascript: false,
        ..Default::default()
    };

    let fetcher = Fetcher::with_config(config).await?;
    let page = fetcher.render_html(&html, base_url.as_deref())?;

    match format {
        OutputFormat::Markdown => {
            println!("{}", page.to_markdown());
        }
        OutputFormat::Plain => {
            println!("{}", page.to_plain_text());
        }
        OutputFormat::Json => {
            let json = serde_json::json!({
                "url": page.url.to_string(),
                "title": page.title,
                "content": page.to_markdown()
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
    }

    Ok(())
}
