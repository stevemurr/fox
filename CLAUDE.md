# Fox - Claude Code Context

## Project Overview

Fox is a terminal web browser written in Rust with vim-style navigation. It's designed as an API-first tool that can be used both as a CLI and as a library.

## Architecture

The project is organized as a Cargo workspace with three crates:

- **fox-core**: Core library with no TUI dependencies
  - `fetch.rs` - HTTP client (reqwest) and headless browser (chromiumoxide)
  - `extract.rs` - Readability-style content extraction
  - `markdown.rs` - HTML to Markdown conversion

- **fox-tui**: Interactive terminal UI
  - `app.rs` - Main application state and event handling
  - `ui.rs` - Ratatui rendering
  - `vim.rs` - Vim mode state machine and command parsing
  - `tabs.rs` - Tab/buffer management
  - `history.rs` - Navigation history with persistence
  - `config.rs` - Configuration loading/saving

- **fox-cli**: Binary entry point
  - `main.rs` - Clap-based CLI with fetch/browse/render subcommands

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| reqwest | HTTP client |
| chromiumoxide | Headless Chrome for JS rendering |
| scraper | HTML parsing + CSS selectors |
| ratatui | TUI framework |
| crossterm | Terminal backend |
| tokio | Async runtime |
| clap | CLI argument parsing |

## Common Tasks

### Building
```bash
cargo build --release
```

### Testing
```bash
cargo test
```

### Running
```bash
# CLI mode
./target/release/fox fetch https://example.com

# Interactive mode
./target/release/fox browse
```

## Code Patterns

- Async/await throughout using tokio runtime
- Error handling via thiserror and anyhow
- Configuration uses serde + toml
- TUI uses ratatui's immediate mode rendering
- Vim modes implemented as state machine in `vim.rs`

## Data Storage

- History: `~/.local/share/fox/history.json`
- Config: `~/.config/fox/config.toml`
- Tabs (session): `~/.local/share/fox/tabs.json`
