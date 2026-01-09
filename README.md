# Grabby - Media Embedding Discord Bot

[![License: GPL3](https://img.shields.io/badge/license-%20%20GNU%20GPLv3%20-blue)](https://github.com/amadejkastelic/grabby/blob/main/LICENSE)
[![Docker Pulls](https://img.shields.io/docker/pulls/amadejkastelic/grabby)](https://hub.docker.com/repository/docker/amadejkastelic/grabby/)

A Discord bot that downloads and embeds media from URLs directly into Discord messages using yt-dlp and gallery-dl. Features slash commands, auto-embed channels, metadata extraction, and user-controlled deletion.

## Features

- **Media Download**: Downloads media from URLs using yt-dlp and gallery-dl (priority order: gallery-dl → yt-dlp)
- **In-Memory Processing**: Downloads media directly to memory and uploads to Discord (no disk I/O)
- **Slash Command**: `/embed` command with options for URL, custom message, and spoiler mode
- **Auto-Embed Channels**: Automatically processes URLs in configured channels without commands
- **Metadata Extraction**: Displays title, author, likes, and original URL with downloaded files
- **File Size Limits**: Enforces Discord's 25MB file size limit with user feedback
- **Auto-Resize**: Automatically resizes oversized media files using ffmpeg to fit Discord's 25MB limit
- **Reaction Deletion**: ❌ emoji reaction allows original poster or admins to delete embeds

## Installation

### Docker

Docker images are available on Docker Hub:

```
https://hub.docker.com/repository/docker/amadejkastelic/grabby
```

Pull the latest image:

```bash
docker pull amadejkastelic/grabby:latest
```

Run the bot:

```bash
docker run -d \
  -v /path/to/config.toml:/config.toml \
  -e DISCORD_TOKEN=your_bot_token_here \
  amadejkastelic/grabby:latest
```

### GitHub Container Registry

Multi-architecture images are also available on ghcr:

```bash
docker pull ghcr.io/amadejkastelic/grabby:latest
```

### Nix

Install and run using Nix flakes:

```bash
# Build the package
nix build .#grabby

# Run the package
nix run .#grabby

# Enter development shell
nix develop

# Build Docker image
nix build .#docker
```

## Configuration

### Config File Locations

Configuration is loaded from one of these locations (in order of priority):

1. CLI argument: `grabby --config /path/to/config.toml`
2. Environment variable: `CONFIG_FILE=/path/to/config.toml`
3. XDG config directory: `$XDG_CONFIG_HOME/grabby/config.toml`
4. Default home directory: `~/.config/grabby/config.toml`

### Config Options

```toml
# Discord configuration (optional)
[discord]
# Bot token (will use DISCORD_TOKEN env var if not set)
# token = "your_bot_token_here"

# Logging configuration (optional)
[logging]
# Format: "json" or "pretty" (default: "json")
format = "json"
# Level: "trace", "debug", "info", "warn", "error" (default: "info")
level = "info"

[[servers]]
server_id = "YOUR_DISCORD_SERVER_ID"
auto_embed_channels = [
  "CHANNEL_ID_1",
  "CHANNEL_ID_2"
]
embed_enabled = true

# Add more servers by repeating the [[servers]] section
# [[servers]]
# server_id = "ANOTHER_SERVER_ID"
# auto_embed_channels = ["CHANNEL_ID_3"]
# embed_enabled = false
```

### Environment Variables

- `DISCORD_TOKEN`: Discord bot token (optional if set in config file)
- `CONFIG_FILE`: Path to config file (optional)

## Usage

### Slash Command

Use the `/embed` command to embed media:

```
/embed url:https://example.com/video.mp4
```

Options:
- `url`: The URL to download and embed
- `message`: Optional custom message to include
- `spoiler`: Mark the content as a spoiler (default: false)

### Auto-Embed Channels

Configure channels for automatic embedding in your config file. Any URL posted in these channels will be automatically embedded without requiring the `/embed` command.

### Reaction Deletion

React with ❌ to delete an embed. Only the message author or users with MANAGE_MESSAGES permission can delete embeds.

## Development

```bash
# Run the bot in development mode
cargo run

# Run tests
cargo test

# Check code formatting
cargo fmt --check

# Run clippy lints
cargo clippy -- -D warnings

# Build release version
cargo build --release
```

### Nix Development

```bash
# Enter development environment with pre-commit hooks
nix develop

# Run pre-commit checks
nix run .#checks.pre-commit-check
```

## Architecture

- **Discord Integration**: Twilight async Discord library with gateway and HTTP clients
- **Media Download**: Abstraction layer supporting multiple downloaders (yt-dlp, gallery-dl)
- **Async Runtime**: Tokio for all async operations
- **Cache**: In-memory caching for Discord objects
- **Config**: Server/channel configuration for auto-embed channels

