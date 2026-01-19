# Accessibility Tree Rendering Implementation Plan

## Overview

Replace the current readability-style content extraction with Chrome's accessibility tree. The AX tree is the semantic representation Chrome builds for screen readers - exactly what we need for "web pages as markdown."

## Current Flow vs New Flow

```
CURRENT:
URL → fetch HTML → readability scoring → HTML parsing → markdown

NEW:
URL → Chrome renders → Accessibility Tree → walk AX nodes → markdown
                                          ↓
                            (fallback: current readability approach for HTTP-only mode)
```

## Part 1: Chrome Distribution Strategy

### Approach: Chrome for Testing (Auto-Download)

Google provides official pre-built Chrome binaries specifically for testing/automation:
- **chrome-headless-shell**: Minimal headless Chrome (~50MB compressed)
- Per-platform builds (mac-x64, mac-arm64, linux64, win64)
- Versioned releases with JSON manifest for discovery

**API Endpoint**: `https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json`

### Download Flow

```
fox needs Chrome
        │
        ▼
┌─────────────────────────┐
│ Check ~/.local/share/   │
│ fox/chrome/             │
└─────────────────────────┘
        │
   Found?───YES──▶ Use cached binary
        │
        NO
        │
        ▼
┌─────────────────────────┐
│ Fetch manifest JSON     │
│ Find latest stable      │
│ Get platform URL        │
└─────────────────────────┘
        │
        ▼
┌─────────────────────────┐
│ Download + extract      │
│ to fox/chrome/          │
│ Mark executable         │
└─────────────────────────┘
        │
        ▼
    Launch Chrome
```

### Storage Layout

```
~/.local/share/fox/
├── chrome/
│   ├── version.txt              # "131.0.6778.87"
│   └── chrome-headless-shell-{platform}/
│       └── chrome-headless-shell  # the binary
├── history.json
└── tabs.json
```

### Platform Detection

```rust
fn get_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("linux", "x86_64") => "linux64",
        ("windows", "x86_64") => "win64",
        _ => panic!("Unsupported platform"),
    }
}
```

### Configuration Schema

```toml
[browser]
# "auto" | "bundled" | "system" | "none"
mode = "auto"

# For system mode: path to Chrome binary (optional, auto-detected if empty)
chrome_path = ""

# Auto-update bundled Chrome (check weekly)
auto_update = true
```

### Priority Resolution (mode = "auto")

```
1. Bundled Chrome (if downloaded)     → fastest, known-good version
2. System Chrome (if found in PATH)   → fallback
3. Download Chrome for Testing        → first-run setup
4. HTTP-only mode                     → graceful degradation
```

---

## Part 2: Accessibility Tree Integration

### CDP Methods Required

chromiumoxide provides raw CDP access. We need:

```
Accessibility.enable()           # Enable AX tree tracking
Accessibility.getFullAXTree()    # Get complete tree
```

### AX Node Structure

Each AX node contains:
```rust
struct AXNode {
    node_id: String,
    role: AXRole,           // heading, link, paragraph, list, listitem, etc.
    name: Option<String>,   // accessible name (text content)
    value: Option<String>,  // for inputs
    properties: Vec<AXProperty>,
    children: Vec<AXNode>,
}
```

### Role-to-Markdown Mapping

| AX Role | Markdown Output |
|---------|-----------------|
| `RootWebArea` | (container, recurse) |
| `heading` (level 1-6) | `# ` to `###### ` |
| `paragraph` | text block + `\n\n` |
| `link` | `[name](url)` |
| `list` | container for items |
| `listitem` | `- ` or `1. ` |
| `code` | backticks |
| `pre` / `preformatted` | ` ``` ` block |
| `blockquote` | `> ` prefix |
| `table` | markdown table |
| `image` | `![name](src)` |
| `navigation`, `banner`, `contentinfo` | **skip** |
| `generic` | recurse into children |

### Implementation: New Module

```
fox-core/src/
├── fetch.rs          (existing)
├── extract.rs        (existing, becomes fallback)
├── markdown.rs       (existing, keep for utilities)
├── accessibility.rs  (NEW: AX tree → markdown)
└── chrome.rs         (NEW: Chrome lifecycle management)
```

---

## Part 3: Chrome Manager Design

### Abstraction Layer

```rust
// fox-core/src/chrome.rs

pub enum ChromeSource {
    Bundled(PathBuf),   // Downloaded chrome-headless-shell
    System(PathBuf),    // Found in PATH or configured
    None,               // HTTP-only fallback
}

pub struct ChromeManager {
    source: ChromeSource,
    browser: Option<Arc<Mutex<Browser>>>,
    data_dir: PathBuf,
}

impl ChromeManager {
    pub async fn new(config: ChromeConfig) -> Result<Self>;
    pub async fn ensure_chrome(&mut self) -> Result<PathBuf>;
    pub async fn get_browser(&mut self) -> Result<&Browser>;
    pub async fn shutdown(&mut self) -> Result<()>;
}
```

### Download Implementation

```rust
impl ChromeManager {
    const MANIFEST_URL: &str =
        "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

    async fn download_chrome(&self) -> Result<PathBuf> {
        // 1. Fetch manifest
        let manifest: Manifest = reqwest::get(Self::MANIFEST_URL)
            .await?.json().await?;

        // 2. Find headless-shell URL for our platform
        let platform = get_platform();
        let channel = &manifest.channels.stable;
        let download = channel.downloads.chrome_headless_shell
            .iter()
            .find(|d| d.platform == platform)
            .ok_or_else(|| anyhow!("No download for {}", platform))?;

        // 3. Download zip
        let zip_path = self.data_dir.join("chrome.zip");
        download_file(&download.url, &zip_path).await?;

        // 4. Extract
        let chrome_dir = self.data_dir.join("chrome");
        extract_zip(&zip_path, &chrome_dir)?;
        std::fs::remove_file(&zip_path)?;

        // 5. Write version marker
        std::fs::write(
            chrome_dir.join("version.txt"),
            &channel.version
        )?;

        // 6. Return binary path
        Ok(chrome_dir.join(format!("chrome-headless-shell-{}", platform))
            .join(if cfg!(windows) { "chrome-headless-shell.exe" } else { "chrome-headless-shell" }))
    }
}
```

### Resolution Logic

```rust
impl ChromeManager {
    pub async fn ensure_chrome(&mut self) -> Result<PathBuf> {
        match &self.source {
            ChromeSource::Bundled(path) | ChromeSource::System(path) => {
                return Ok(path.clone());
            }
            ChromeSource::None => {}
        }

        // Check for existing bundled Chrome
        let chrome_dir = self.data_dir.join("chrome");
        if let Some(path) = self.find_bundled_chrome(&chrome_dir) {
            self.source = ChromeSource::Bundled(path.clone());
            return Ok(path);
        }

        // Check system PATH
        if let Some(path) = find_system_chrome() {
            self.source = ChromeSource::System(path.clone());
            return Ok(path);
        }

        // Download Chrome for Testing
        info!("Downloading Chrome for Testing...");
        let path = self.download_chrome().await?;
        self.source = ChromeSource::Bundled(path.clone());
        Ok(path)
    }

    fn find_bundled_chrome(&self, chrome_dir: &Path) -> Option<PathBuf> {
        let platform = get_platform();
        let binary = if cfg!(windows) { "chrome-headless-shell.exe" } else { "chrome-headless-shell" };
        let path = chrome_dir
            .join(format!("chrome-headless-shell-{}", platform))
            .join(binary);
        path.exists().then_some(path)
    }
}

fn find_system_chrome() -> Option<PathBuf> {
    // Check common locations
    let candidates = if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
        ]
    } else {
        vec![
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    };

    candidates.into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .or_else(|| which::which("chrome").ok())
        .or_else(|| which::which("chromium").ok())
}
```

---

## Part 4: Implementation Phases

### Phase 1: Chrome Manager + Auto-Download
1. Create `chrome.rs` with `ChromeManager` struct
2. Implement Chrome for Testing manifest parsing + download
3. Add platform detection and binary resolution
4. Move browser initialization from `fetch.rs` to `ChromeManager`
5. Add configuration schema for browser settings
6. Test: system Chrome, bundled Chrome, auto-download

### Phase 2: Accessibility Tree Extraction
1. Create `accessibility.rs` module
2. Implement CDP calls for AX tree retrieval via chromiumoxide
3. Define Rust structs for AX nodes
4. Test raw AX tree extraction on sample pages

### Phase 3: AX-to-Markdown Conversion
1. Implement role-to-markdown mapping
2. Handle link URL extraction (DOM cross-reference for hrefs)
3. Handle tables, lists, nested structures
4. Clean up output (excessive whitespace, etc.)
5. Handle edge cases: empty nodes, deeply nested content

### Phase 4: Integration + Polish
1. Wire `accessibility.rs` into main fetch pipeline
2. Keep `extract.rs` as fallback for HTTP-only mode
3. Add config toggle: `extraction_method = "accessibility" | "readability"`
4. Progress indicator for Chrome download
5. Performance testing and optimization

---

## Part 5: Key Technical Decisions

### Q1: How to get link URLs from AX tree?

The AX tree `name` property contains link text, but URLs require DOM cross-reference:
```rust
// Option A: CDP call to get node's href
page.execute(GetAttributes { node_id }).await

// Option B: Build URL map from DOM first, cross-reference by node_id
let dom = page.execute(DOM.getDocument).await;
// ... build map ...
```

**Recommendation**: Option B - single DOM traversal, then pure AX tree walk.

### Q2: How to handle dynamic content?

Wait strategies:
1. `waitForNavigation` (current)
2. `waitForSelector` with visibility check
3. Custom idle detection (network idle + DOM stable)

**Recommendation**: Add configurable wait strategy, default to network idle.

### Q3: First-run UX for Chrome download?

Options:
1. Silent download on first fetch (may surprise user with 50MB download)
2. Prompt user before download
3. Require explicit `fox setup` command

**Recommendation**: Option 1 with clear progress logging. The download is small (~50MB) and one-time.

---

## Part 6: Fallback Strategy

```
┌─────────────────────────────────────────────────────────┐
│                    fetch(url)                           │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
            ┌─────────────────────────┐
            │  Chrome available?       │
            └─────────────────────────┘
                    │           │
                   YES          NO
                    │           │
                    ▼           ▼
        ┌───────────────┐   ┌───────────────────────┐
        │ Render page   │   │ HTTP fetch            │
        │ Get AX tree   │   │ Readability extract   │
        │ AX→Markdown   │   │ HTML→Markdown         │
        └───────────────┘   └───────────────────────┘
                    │           │
                    └─────┬─────┘
                          ▼
                    ExtractedContent
```

---

## Part 7: Open Questions for Discussion

1. **Chrome for Testing vs full Chrome**:
   - `chrome-headless-shell`: Minimal (~50MB), purpose-built for automation
   - `chrome`: Full browser (~130MB), supports headed mode for debugging
   - **Recommendation**: chrome-headless-shell for size, with config option to use full Chrome

2. **Version pinning strategy**:
   - Always latest stable? (auto-update)
   - Pin to tested version in code?
   - User-configurable version?
   - **Recommendation**: Default to latest stable, with `auto_update = false` option

3. **chromiumoxide vs alternatives**:
   - `chromiumoxide`: Already integrated, mature, async-first
   - `headless_chrome`: Simpler API, less maintained
   - `fantoccini`: WebDriver-based (different protocol)
   - **Recommendation**: Stick with chromiumoxide, it works well

4. **Testing strategy**:
   - Mock AX trees for unit tests
   - Integration tests with real Chrome (CI has Chrome)
   - Reference pages for regression testing
   - **Recommendation**: Mock for unit tests, real Chrome for integration
