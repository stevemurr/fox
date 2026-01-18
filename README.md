# Fox

A hyper-modern, API-first terminal web browser with vim-mode navigation.

## Features

- **API-first design** - Use as a CLI tool or library
- **Vim-style navigation** - Familiar keybindings for efficient browsing
- **JavaScript support** - Headless Chrome rendering via chromiumoxide
- **Reader mode** - Readability-style content extraction
- **Multiple output formats** - Markdown, plain text, JSON
- **Tab management** - Multiple buffers with vim-style switching
- **History navigation** - Back/forward with persistent history

## Installation

```bash
cargo install --path crates/fox-cli
```

Or build from source:

```bash
cargo build --release
./target/release/fox --help
```

## Usage

### Fetch Mode (API-first)

```bash
# Fetch webpage as markdown
fox fetch https://example.com

# Skip JavaScript rendering (faster, HTTP only)
fox fetch https://example.com --no-js

# Output as JSON with extracted links
fox fetch https://example.com --format json

# Output as plain text
fox fetch https://example.com --format plain

# Pipe-friendly
fox fetch https://news.ycombinator.com | grep "Rust"
```

### Render Mode

```bash
# Render HTML from stdin
echo "<h1>Hello</h1><p>World</p>" | fox render

# With base URL for resolving links
curl -s https://example.com | fox render --base-url https://example.com
```

### Browse Mode (Interactive TUI)

```bash
# Open interactive browser
fox browse https://example.com

# Open with blank page
fox browse
```

## Keybindings

### Normal Mode

| Key | Action |
|-----|--------|
| `j/k` | Scroll down/up |
| `gg` | Go to top |
| `G` | Go to bottom |
| `Ctrl-d/u` | Half-page down/up |
| `Ctrl-f/b` | Full page down/up |
| `f` | Follow link (hint mode) |
| `h/l` | Previous/next link |
| `H` | Go back in history |
| `L` | Go forward in history |
| `/` | Search page |
| `n/N` | Next/prev search result |
| `y` | Yank current URL |
| `p` | Open URL from clipboard |

### Command Mode

| Command | Action |
|---------|--------|
| `:o <url>` | Open URL in current tab |
| `:t <url>` | Open URL in new tab |
| `:q` | Quit |
| `:w <file>` | Save page as markdown |
| `:tabs` | List open tabs |
| `:history` | Show history |
| `:set <key>=<value>` | Change settings |

### Tab Management

| Key | Action |
|-----|--------|
| `gt` | Next tab |
| `gT` | Previous tab |
| `1gt`-`9gt` | Go to tab N |
| `d` | Close current tab |

## Configuration

Configuration is stored in `~/.config/fox/config.toml`:

```toml
[general]
default_mode = "reader"    # reader | full
javascript = true          # Enable JS rendering
timeout_secs = 30

[display]
max_width = 80             # Text wrap width
show_links = "inline"      # inline | footnote | hidden
show_images = true
```

## Architecture

```
fox/
├── crates/
│   ├── fox-core/      # Core library (fetch, extract, markdown)
│   ├── fox-tui/       # Interactive terminal UI
│   └── fox-cli/       # CLI entry point
└── config/            # Default configuration
```

## Requirements

- Rust 1.70+
- For JavaScript rendering: Chrome/Chromium installed

## License

MIT
