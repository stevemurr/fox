//! Fox CLI - Terminal Web Browser
//!
//! A hyper-modern, API-first terminal web browser with vim-mode navigation.

use anyhow::Result;
use clap::{Parser, Subcommand};
use fox_core::{fetch::Fetcher, FetchConfig};
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
        Some(Commands::Fetch { url, format, no_js }) => {
            run_fetch(&url, format, no_js).await?;
        }
        Some(Commands::Browse { url }) => {
            run_browse(url).await?;
        }
        Some(Commands::Render { base_url, format }) => {
            run_render(base_url, format).await?;
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

async fn run_fetch(url: &str, format: OutputFormat, no_js: bool) -> Result<()> {
    let config = FetchConfig {
        javascript: !no_js,
        ..Default::default()
    };

    let fetcher = Fetcher::with_config(config).await?;
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
