//! Chrome browser lifecycle management
//!
//! Handles Chrome discovery, auto-download, and browser initialization.
//! Supports bundled Chrome for Testing, system Chrome, or HTTP-only fallback.

use crate::{FoxError, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Progress update during Chrome download
#[derive(Debug, Clone)]
pub enum DownloadProgress {
    /// Starting the download
    Starting { version: String },
    /// Download in progress (bytes downloaded, total bytes if known)
    Downloading { downloaded: u64, total: Option<u64> },
    /// Extracting the archive
    Extracting,
    /// Download complete
    Complete { path: PathBuf },
    /// Download failed
    Failed { error: String },
}

/// Callback type for download progress updates
pub type ProgressCallback = Box<dyn Fn(DownloadProgress) + Send + Sync>;

/// Where Chrome was resolved from
#[derive(Debug, Clone)]
pub enum ChromeSource {
    /// Downloaded chrome-headless-shell
    Bundled(PathBuf),
    /// Found in system PATH or configured location
    System(PathBuf),
    /// No Chrome available, HTTP-only mode
    None,
}

/// Extraction method for converting pages to markdown
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionMethod {
    /// Use Chrome's accessibility tree (requires JS rendering)
    #[default]
    Accessibility,
    /// Use readability-style content extraction (works with HTTP-only)
    Readability,
}

impl std::str::FromStr for ExtractionMethod {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "accessibility" | "ax" | "a11y" => Ok(ExtractionMethod::Accessibility),
            "readability" | "reader" => Ok(ExtractionMethod::Readability),
            _ => Err(format!("Unknown extraction method: {}. Use 'accessibility' or 'readability'", s)),
        }
    }
}

impl std::fmt::Display for ExtractionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractionMethod::Accessibility => write!(f, "accessibility"),
            ExtractionMethod::Readability => write!(f, "readability"),
        }
    }
}

/// Configuration for Chrome browser management
#[derive(Debug, Clone)]
pub struct ChromeConfig {
    /// Browser mode: "auto", "bundled", "system", or "none"
    pub mode: String,
    /// Custom Chrome binary path (for system mode)
    pub chrome_path: Option<PathBuf>,
    /// Data directory for storing bundled Chrome
    pub data_dir: PathBuf,
    /// Whether to auto-update bundled Chrome
    pub auto_update: bool,
    /// Content extraction method
    pub extraction_method: ExtractionMethod,
}

impl Default for ChromeConfig {
    fn default() -> Self {
        let data_dir = directories::ProjectDirs::from("", "", "fox")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".fox"));

        Self {
            mode: "auto".to_string(),
            chrome_path: None,
            data_dir,
            auto_update: true,
            extraction_method: ExtractionMethod::default(),
        }
    }
}

/// Handle to a running browser instance
pub struct BrowserHandle {
    pub browser: Browser,
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
}

/// Manages Chrome lifecycle: discovery, download, and browser instances
pub struct ChromeManager {
    config: ChromeConfig,
    source: ChromeSource,
    browser: Option<Arc<Mutex<BrowserHandle>>>,
    progress_callback: Option<Arc<ProgressCallback>>,
}

impl ChromeManager {
    /// Chrome for Testing manifest URL
    const MANIFEST_URL: &'static str =
        "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

    /// Create a new ChromeManager with default configuration
    pub fn new() -> Self {
        Self::with_config(ChromeConfig::default())
    }

    /// Create a new ChromeManager with custom configuration
    pub fn with_config(config: ChromeConfig) -> Self {
        Self {
            config,
            source: ChromeSource::None,
            browser: None,
            progress_callback: None,
        }
    }

    /// Set a callback for download progress updates
    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: Fn(DownloadProgress) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(Box::new(callback)));
    }

    /// Report progress to the callback if set
    fn report_progress(&self, progress: DownloadProgress) {
        if let Some(ref callback) = self.progress_callback {
            callback(progress);
        }
    }

    /// Get the current Chrome source
    pub fn source(&self) -> &ChromeSource {
        &self.source
    }

    /// Ensure Chrome is available, downloading if necessary
    pub async fn ensure_chrome(&mut self) -> Result<PathBuf> {
        // If already resolved, return cached path
        if let ChromeSource::Bundled(ref path) | ChromeSource::System(ref path) = self.source {
            if path.exists() {
                return Ok(path.clone());
            }
        }

        // Resolve based on mode
        match self.config.mode.as_str() {
            "none" => {
                self.source = ChromeSource::None;
                return Err(FoxError::BrowserError(
                    "Chrome disabled by configuration".to_string(),
                ));
            }
            "system" => {
                if let Some(ref path) = self.config.chrome_path {
                    if path.exists() {
                        self.source = ChromeSource::System(path.clone());
                        return Ok(path.clone());
                    }
                }
                if let Some(path) = find_system_chrome() {
                    self.source = ChromeSource::System(path.clone());
                    return Ok(path);
                }
                return Err(FoxError::BrowserError(
                    "No system Chrome found".to_string(),
                ));
            }
            "bundled" => {
                if let Some(path) = self.find_bundled_chrome() {
                    self.source = ChromeSource::Bundled(path.clone());
                    return Ok(path);
                }
                return self.download_chrome().await;
            }
            "auto" | _ => {
                // Try bundled first (fastest, known-good)
                if let Some(path) = self.find_bundled_chrome() {
                    debug!("Using bundled Chrome: {:?}", path);
                    self.source = ChromeSource::Bundled(path.clone());
                    return Ok(path);
                }

                // Try system Chrome
                if let Some(path) = find_system_chrome() {
                    debug!("Using system Chrome: {:?}", path);
                    self.source = ChromeSource::System(path.clone());
                    return Ok(path);
                }

                // Download Chrome for Testing
                info!("No Chrome found, downloading Chrome for Testing...");
                return self.download_chrome().await;
            }
        }
    }

    /// Find bundled Chrome in the data directory
    fn find_bundled_chrome(&self) -> Option<PathBuf> {
        let chrome_dir = self.config.data_dir.join("chrome");
        let platform = get_platform();
        let binary_name = if cfg!(windows) {
            "chrome-headless-shell.exe"
        } else {
            "chrome-headless-shell"
        };

        let path = chrome_dir
            .join(format!("chrome-headless-shell-{}", platform))
            .join(binary_name);

        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Download Chrome for Testing
    async fn download_chrome(&mut self) -> Result<PathBuf> {
        let platform = get_platform();
        info!("Downloading Chrome for Testing for {}...", platform);

        // Fetch manifest
        let client = reqwest::Client::new();
        let manifest: ChromeManifest = client
            .get(Self::MANIFEST_URL)
            .send()
            .await
            .map_err(|e| FoxError::BrowserError(format!("Failed to fetch Chrome manifest: {}", e)))?
            .json()
            .await
            .map_err(|e| FoxError::BrowserError(format!("Failed to parse Chrome manifest: {}", e)))?;

        // Find headless-shell download for our platform
        let channel = &manifest.channels.stable;
        let download = channel
            .downloads
            .chrome_headless_shell
            .iter()
            .find(|d| d.platform == platform)
            .ok_or_else(|| {
                FoxError::BrowserError(format!("No Chrome download available for {}", platform))
            })?;

        info!("Downloading Chrome {} from {}", channel.version, download.url);
        self.report_progress(DownloadProgress::Starting {
            version: channel.version.clone(),
        });

        // Create chrome directory
        let chrome_dir = self.config.data_dir.join("chrome");
        std::fs::create_dir_all(&chrome_dir)?;

        // Download the zip with progress reporting
        let zip_path = chrome_dir.join("chrome.zip");
        self.download_file_with_progress(&client, &download.url, &zip_path)
            .await?;

        // Extract
        info!("Extracting Chrome...");
        self.report_progress(DownloadProgress::Extracting);
        extract_zip(&zip_path, &chrome_dir)?;

        // Remove zip file
        std::fs::remove_file(&zip_path)?;

        // Write version marker
        std::fs::write(chrome_dir.join("version.txt"), &channel.version)?;

        // Find the binary
        let binary_name = if cfg!(windows) {
            "chrome-headless-shell.exe"
        } else {
            "chrome-headless-shell"
        };

        let binary_path = chrome_dir
            .join(format!("chrome-headless-shell-{}", platform))
            .join(binary_name);

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&binary_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&binary_path, perms)?;
        }

        info!("Chrome installed successfully at {:?}", binary_path);
        self.report_progress(DownloadProgress::Complete {
            path: binary_path.clone(),
        });
        self.source = ChromeSource::Bundled(binary_path.clone());
        Ok(binary_path)
    }

    /// Download a file with progress reporting
    async fn download_file_with_progress(
        &self,
        client: &reqwest::Client,
        url: &str,
        path: &Path,
    ) -> Result<()> {
        use futures::TryStreamExt;
        use tokio::io::AsyncWriteExt;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| FoxError::BrowserError(format!("Download failed: {}", e)))?;

        let total_size = response.content_length();
        let mut downloaded: u64 = 0;

        // Report initial progress
        self.report_progress(DownloadProgress::Downloading {
            downloaded: 0,
            total: total_size,
        });

        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|e| FoxError::IoError(e))?;

        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream
            .try_next()
            .await
            .map_err(|e| FoxError::BrowserError(format!("Failed to read download: {}", e)))?
        {
            file.write_all(&chunk)
                .await
                .map_err(|e| FoxError::IoError(e))?;

            downloaded += chunk.len() as u64;

            // Report progress (throttle to avoid too many updates)
            if downloaded % (1024 * 100) < chunk.len() as u64 || total_size.map(|t| downloaded >= t).unwrap_or(false) {
                self.report_progress(DownloadProgress::Downloading {
                    downloaded,
                    total: total_size,
                });
            }
        }

        file.flush().await.map_err(|e| FoxError::IoError(e))?;
        Ok(())
    }

    /// Get or create a browser instance
    pub async fn get_browser(&mut self) -> Result<Arc<Mutex<BrowserHandle>>> {
        // Return existing browser if available
        if let Some(ref browser) = self.browser {
            return Ok(Arc::clone(browser));
        }

        // Ensure Chrome is available
        let chrome_path = self.ensure_chrome().await?;

        // Launch browser
        let handle = launch_browser(&chrome_path).await?;
        let browser = Arc::new(Mutex::new(handle));
        self.browser = Some(Arc::clone(&browser));

        Ok(browser)
    }

    /// Check if Chrome is available (without downloading)
    pub fn is_chrome_available(&self) -> bool {
        matches!(
            self.source,
            ChromeSource::Bundled(_) | ChromeSource::System(_)
        ) || self.find_bundled_chrome().is_some()
            || find_system_chrome().is_some()
    }

    /// Shutdown the browser
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(browser) = self.browser.take() {
            // Browser will be dropped, which triggers cleanup
            drop(browser);
        }
        Ok(())
    }
}

impl Default for ChromeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Launch a browser instance with the given Chrome path
async fn launch_browser(chrome_path: &Path) -> Result<BrowserHandle> {
    debug!("Launching browser from {:?}", chrome_path);

    let (browser, mut handler) = Browser::launch(
        BrowserConfig::builder()
            .chrome_executable(chrome_path)
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-software-rasterizer")
            .build()
            .map_err(|e| FoxError::BrowserError(e.to_string()))?,
    )
    .await
    .map_err(|e| FoxError::BrowserError(format!("Failed to launch browser: {}", e)))?;

    let handle = tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    Ok(BrowserHandle { browser, handle })
}

/// Get the platform identifier for Chrome for Testing downloads
fn get_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("linux", "x86_64") => "linux64",
        ("windows", "x86_64") => "win64",
        (os, arch) => {
            warn!("Unsupported platform: {}-{}, trying linux64", os, arch);
            "linux64"
        }
    }
}

/// Find Chrome installed on the system
pub fn find_system_chrome() -> Option<PathBuf> {
    let candidates: Vec<&str> = if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    } else {
        vec![]
    };

    // Check hardcoded paths first
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    // Try PATH
    which::which("google-chrome")
        .or_else(|_| which::which("google-chrome-stable"))
        .or_else(|_| which::which("chromium"))
        .or_else(|_| which::which("chromium-browser"))
        .ok()
}

/// Extract a zip file to a directory
fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| FoxError::BrowserError(format!("Failed to open zip: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| FoxError::BrowserError(format!("Failed to read zip entry: {}", e)))?;

        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // Set permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }

    Ok(())
}

// ============================================================================
// Chrome for Testing Manifest Types
// ============================================================================

#[derive(Debug, Deserialize)]
struct ChromeManifest {
    channels: Channels,
}

#[derive(Debug, Deserialize)]
struct Channels {
    #[serde(rename = "Stable")]
    stable: Channel,
}

#[derive(Debug, Deserialize)]
struct Channel {
    version: String,
    downloads: Downloads,
}

#[derive(Debug, Deserialize)]
struct Downloads {
    #[serde(rename = "chrome-headless-shell")]
    chrome_headless_shell: Vec<Download>,
}

#[derive(Debug, Deserialize)]
struct Download {
    platform: String,
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_platform() {
        let platform = get_platform();
        // Should return a valid platform string
        assert!(
            ["mac-arm64", "mac-x64", "linux64", "win64"].contains(&platform),
            "Unexpected platform: {}",
            platform
        );
    }

    #[test]
    fn test_chrome_config_default() {
        let config = ChromeConfig::default();
        assert_eq!(config.mode, "auto");
        assert!(config.chrome_path.is_none());
        assert!(config.auto_update);
    }

    #[test]
    fn test_find_system_chrome() {
        // This test just checks that the function doesn't panic
        let _result = find_system_chrome();
    }
}
