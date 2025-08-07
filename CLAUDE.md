# Grabby - Media Embedding Discord Bot

A Rust Discord bot that downloads and embeds media from URLs using yt-dlp. Built with Twilight for Discord API integration and designed to be extensible for Slack and Telegram support.

## Architecture
- **Discord Integration**: Twilight async Discord library
- **Media Download**: yt-dlp wrapper for Rust
- **Async Runtime**: Tokio
- **Future Platforms**: Slack, Telegram

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
- `DISCORD_TOKEN`: Discord bot token (required)
- `LOG_LEVEL`: Log level (default: info)

## Project Structure
- `src/main.rs`: Entry point and bot initialization
- `src/bot/`: Discord bot implementation
- `src/media/`: Media download and processing
- `src/config/`: Configuration management
- `src/utils/`: Utility functions

## Dependencies
- `twilight-*`: Discord API client and utilities
- `tokio`: Async runtime
- `yt-dlp`: Media download wrapper
- `serde`: Serialization
- `tracing`: Logging
- `anyhow`: Error handling