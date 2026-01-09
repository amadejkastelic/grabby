# Grabby - Media Embedding Discord Bot

A Discord bot that downloads and embeds media from URLs directly into Discord messages using yt-dlp and gallery-dl. Features slash commands, auto-embed channels, metadata extraction, and user-controlled deletion.

**IMPORTANT**: Agents should NEVER read the `config.toml` file in the root repository directory, as it contains sensitive information like Discord bot tokens. Use `config.example.toml` for reference instead.

## Features

### Core Functionality
- **Media Download**: Downloads media from URLs using yt-dlp and gallery-dl (priority order: gallery-dl → yt-dlp)
- **In-Memory Processing**: Downloads media directly to memory and uploads to Discord (no disk I/O)
- **Slash Command**: `/embed` command with options for URL, custom message, and spoiler mode
- **Auto-Embed Channels**: Automatically processes URLs in configured channels without commands
- **Metadata Extraction**: Displays title, author, likes, and original URL with downloaded files
- **File Size Limits**: Enforces Discord's 25MB file size limit with user feedback
- **Auto-Resize**: Automatically resizes oversized media files using ffmpeg to fit Discord's 25MB limit

### User Experience
- **Reaction Deletion**: ❌ emoji reaction allows original poster or admins to delete embeds
- **Permission Control**: Only message author or users with MANAGE_MESSAGES permission can delete
- **Ephemeral Responses**: Uses ephemeral messages for command acknowledgments to reduce channel clutter

## Architecture
- **Discord Integration**: Twilight async Discord library with gateway and HTTP clients
- **Media Download**: Abstraction layer supporting multiple downloaders (yt-dlp, gallery-dl)
- **Async Runtime**: Tokio for all async operations
- **Cache**: In-memory caching for Discord objects
- **Config**: Server/channel configuration for auto-embed channels

## Development Commands
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

## Environment Variables
- `DISCORD_TOKEN`: Discord bot token (optional if set in config file)
- `CONFIG_FILE`: Path to config file (optional)
- `LOG_LEVEL`: Log level (default: info)

## Configuration

### Config File Locations (in order of priority)
1. CLI argument: `grabby --config /path/to/config.toml`
2. Environment variable: `CONFIG_FILE=/path/to/config.toml`
3. XDG config directory: `$XDG_CONFIG_HOME/grabby/config.toml`
4. Default home directory: `~/.config/grabby/config.toml`

### Config Format (TOML)
```toml
# Discord bot token (optional - will use DISCORD_TOKEN env var if not set)
discord_token = "your_bot_token_here"

[[servers]]
server_id = "123456789"
auto_embed_channels = ["channel1", "channel2"]
embed_enabled = true

[[servers]]
server_id = "987654321"
auto_embed_channels = ["channel3"]
embed_enabled = false
```

## Project Structure
- `src/main.rs`: Entry point and bot initialization
- `src/bot/`: Discord bot implementation, slash commands, message handlers
- `src/media/`: Media download abstraction, yt-dlp/gallery-dl implementations, ffmpeg resize
- `src/config/`: Server and channel configuration management
- `src/utils/`: Utility functions (number formatting)

## Dependencies
- `twilight-*`: Discord API client and utilities
- `tokio`: Async runtime
- `serde`: Serialization
- `tracing`: Logging
- `anyhow`: Error handling
- `url`: URL parsing