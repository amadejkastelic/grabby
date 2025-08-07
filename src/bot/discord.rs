use crate::{config::ConfigManager, media::MediaDownloader};
use anyhow::{Context, Result};
use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{error, info, warn};
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::{Event, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client as HttpClient;
use twilight_model::{
    application::{
        command::CommandType,
        interaction::{
            application_command::CommandData, Interaction, InteractionData, InteractionType,
        },
    },
    channel::message::MessageFlags,
    gateway::payload::incoming::MessageCreate,
    http::{
        attachment::Attachment,
        interaction::{InteractionResponse, InteractionResponseType},
    },
    id::{marker::ChannelMarker, Id},
};
use twilight_util::builder::command::{CommandBuilder, StringBuilder};

pub struct DiscordBot {
    http: HttpClient,
    cache: InMemoryCache,
    shard: Shard,
    media_downloader: MediaDownloader,
    config: ConfigManager,
    application_id: Id<twilight_model::id::marker::ApplicationMarker>,
}

impl DiscordBot {
    pub async fn new(token: String) -> Result<Self> {
        let http = HttpClient::new(token.clone());
        let cache = InMemoryCache::new();

        let intents = Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT;
        let shard = Shard::new(ShardId::ONE, token, intents);

        let media_downloader =
            MediaDownloader::new().context("Failed to initialize media downloader")?;

        // Test the media downloader setup
        if let Err(e) = media_downloader.test_setup().await {
            warn!("Media downloader test failed: {}", e);
        }

        let config = ConfigManager::new();

        // Get application ID
        let application_id = {
            let response = http.current_user_application().await?;
            response.model().await?.id
        };

        let bot = Self {
            http,
            cache,
            shard,
            media_downloader,
            config,
            application_id,
        };

        // Register slash commands
        bot.register_commands().await?;

        Ok(bot)
    }

    async fn register_commands(&self) -> Result<()> {
        info!("Registering Discord slash commands...");

        // Build the /embed command
        let embed_command = CommandBuilder::new(
            "embed".to_string(),
            "Download and embed media from a URL".to_string(),
            CommandType::ChatInput,
        )
        .option(StringBuilder::new("url", "URL to download and embed").required(true))
        .build();

        // Create the global command using the interaction client
        self.http
            .interaction(self.application_id)
            .create_global_command()
            .chat_input(&embed_command.name, &embed_command.description)
            .command_options(&embed_command.options)
            .await?;

        info!("Successfully registered /embed slash command");
        Ok(())
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Discord bot starting...");

        loop {
            let event = match self
                .shard
                .next_event(twilight_gateway::EventTypeFlags::all())
                .await
            {
                Some(Ok(event)) => event,
                Some(Err(source)) => {
                    error!(?source, "Error receiving event");
                    continue;
                }
                None => {
                    info!("Shard stream ended");
                    return Ok(());
                }
            };

            self.cache.update(&event);

            match event {
                Event::MessageCreate(msg) => {
                    self.handle_message(&msg).await?;
                }
                Event::InteractionCreate(interaction) => {
                    self.handle_interaction(&interaction).await?;
                }
                Event::Ready(_) => {
                    info!("Discord bot is ready!");
                }
                _ => {}
            }
        }
    }

    async fn handle_message(&self, msg: &MessageCreate) -> Result<()> {
        // Skip bot messages
        if msg.author.bot {
            return Ok(());
        }

        // Check if this is an auto-embed channel
        if let Some(guild_id) = msg.guild_id {
            if self
                .config
                .is_auto_embed_channel(&guild_id.to_string(), &msg.channel_id.to_string())
            {
                // Extract URLs from message content and process them
                for url in self.extract_urls(&msg.content) {
                    if self.media_downloader.is_supported_url(&url) {
                        match self.media_downloader.download(&url).await {
                            Ok(media_info) => {
                                info!("Downloaded media: {}", media_info.metadata.title);
                                if let Err(e) = self
                                    .send_media_to_channel(&msg.channel_id, &media_info)
                                    .await
                                {
                                    error!("Failed to send media to channel: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to download media from {}: {}", url, e);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::single_match)]
    async fn handle_interaction(&self, interaction: &Interaction) -> Result<()> {
        match interaction.kind {
            InteractionType::ApplicationCommand => {
                if let Some(InteractionData::ApplicationCommand(data)) = &interaction.data {
                    match data.name.as_str() {
                        "embed" => {
                            self.handle_embed_command(interaction, data).await?;
                        }
                        _ => {
                            info!("Unknown command: {}", data.name);
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_embed_command(
        &self,
        interaction: &Interaction,
        data: &CommandData,
    ) -> Result<()> {
        // Extract URL from command options
        let url = data.options.iter()
            .find(|opt| opt.name == "url")
            .and_then(|opt| match &opt.value {
                twilight_model::application::interaction::application_command::CommandOptionValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("");

        if url.is_empty() {
            self.respond_to_interaction(interaction, "Please provide a valid URL.")
                .await?;
            return Ok(());
        }

        if !self.media_downloader.is_supported_url(url) {
            self.respond_to_interaction(interaction, "This URL is not supported.")
                .await?;
            return Ok(());
        }

        // Acknowledge the interaction first
        self.respond_to_interaction(interaction, "Downloading media...")
            .await?;

        // Download and process the media
        match self.media_downloader.download(url).await {
            Ok(media_info) => {
                info!("Successfully downloaded: {}", media_info.metadata.title);

                if let Some(_file_path) = &media_info.file_path {
                    // Use the working channel upload method instead of interaction followup
                    let channel_id = match interaction.channel.as_ref() {
                        Some(channel) => channel.id,
                        None => {
                            error!("No channel information in interaction");
                            let _ = self
                                .followup_message(
                                    interaction,
                                    "❌ Cannot determine channel for upload",
                                )
                                .await;
                            return Ok(());
                        }
                    };

                    if let Err(e) = self.send_media_to_channel(&channel_id, &media_info).await {
                        error!("Failed to send media to channel: {}", e);
                        let _ = self
                            .followup_message(interaction, "❌ Failed to send media file")
                            .await;
                    }
                } else {
                    let _ = self
                        .followup_message(interaction, "✅ Media processed but no file to send")
                        .await;
                }
            }
            Err(e) => {
                error!("Failed to download media: {}", e);
                let _ = self
                    .followup_message(interaction, &format!("❌ Download failed: {e}"))
                    .await;
            }
        }

        Ok(())
    }

    async fn respond_to_interaction(&self, interaction: &Interaction, content: &str) -> Result<()> {
        let response = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(twilight_model::http::interaction::InteractionResponseData {
                allowed_mentions: None,
                attachments: None,
                choices: None,
                components: None,
                content: Some(content.to_string()),
                custom_id: None,
                embeds: None,
                flags: Some(MessageFlags::EPHEMERAL),
                title: None,
                tts: None,
            }),
        };

        self.http
            .interaction(self.application_id)
            .create_response(interaction.id, &interaction.token, &response)
            .await?;

        Ok(())
    }

    async fn followup_message(&self, interaction: &Interaction, content: &str) -> Result<()> {
        self.http
            .interaction(self.application_id)
            .create_followup(&interaction.token)
            .content(content)
            .await?;
        Ok(())
    }

    async fn send_media_to_channel(
        &self,
        channel_id: &Id<ChannelMarker>,
        media_info: &crate::media::MediaInfo,
    ) -> Result<()> {
        if let Some(file_path) = &media_info.file_path {
            let file_size = std::fs::metadata(file_path)?.len();

            // Discord has a 25MB file size limit for most servers
            if file_size > 25_000_000 {
                self.http
                    .create_message(*channel_id)
                    .content(&format!(
                        "❌ **{}** - File too large ({:.1}MB). Discord limit is 25MB.",
                        media_info.metadata.title,
                        file_size as f64 / 1_000_000.0
                    ))
                    .await?;
                return Ok(());
            };
            let file_name = "media.mp4";

            let attachment = Attachment::from_bytes(
                file_name.to_string(),
                std::fs::read(file_path)?,
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            );

            let content = format!("🎬 **{}**", media_info.url);

            self.http
                .create_message(*channel_id)
                .content(&content)
                .attachments(&[attachment])
                .await?;
        }

        Ok(())
    }

    fn extract_urls(&self, content: &str) -> Vec<String> {
        content
            .split_whitespace()
            .filter_map(|word| {
                if word.starts_with("http://") || word.starts_with("https://") {
                    Some(word.to_string())
                } else {
                    None
                }
            })
            .collect()
    }
}

pub async fn run() -> Result<()> {
    let token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN environment variable is required");

    let bot = DiscordBot::new(token).await?;
    bot.run().await
}
