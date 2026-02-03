use crate::{config::ConfigManager, media::MediaDownloader};
use anyhow::{Context, Result};
use std::env;
use std::sync::Arc;
use tokio::join;
use tracing::{debug, error, info, warn};
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::{Event, Intents, Shard, ShardId, StreamExt};
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_http::Client as HttpClient;
use twilight_model::{
    application::{
        command::CommandType,
        interaction::{
            application_command::CommandData, Interaction, InteractionData, InteractionType,
        },
    },
    channel::message::{EmojiReactionType, MessageFlags},
    gateway::payload::incoming::{MessageCreate, ReactionAdd},
    http::{
        attachment::Attachment,
        interaction::{InteractionResponse, InteractionResponseType},
    },
    id::{
        marker::{ApplicationMarker, ChannelMarker, UserMarker},
        Id,
    },
};
use twilight_util::builder::command::{BooleanBuilder, CommandBuilder, StringBuilder};

fn clean_error_message(error: &anyhow::Error) -> String {
    let error_str = error.to_string().to_lowercase();

    if error_str.contains("unsupported url") || error_str.contains("no extractor found") {
        return "Unsupported URL".to_string();
    }

    if error_str.contains("network error") || error_str.contains("connection") {
        return "Network error - please try again".to_string();
    }

    if error_str.contains("timeout") {
        return "Request timed out - please try again".to_string();
    }

    "Download failed".to_string()
}

#[derive(Clone)]
pub struct DiscordBot {
    http: Arc<HttpClient>,
    cache: Arc<InMemoryCache>,
    media_downloader: Arc<MediaDownloader>,
    config: Arc<ConfigManager>,
    application_id: Id<ApplicationMarker>,
    user_id: Id<UserMarker>,
}

impl DiscordBot {
    pub async fn new(token: String) -> Result<(Self, Shard)> {
        Self::new_with_config(token, ConfigManager::new()).await
    }

    pub async fn new_with_config(token: String, config: ConfigManager) -> Result<(Self, Shard)> {
        let http = Arc::new(HttpClient::new(token.clone()));
        let cache = Arc::new(InMemoryCache::new());

        let intents =
            Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT | Intents::GUILD_MESSAGE_REACTIONS;
        let shard = Shard::new(ShardId::ONE, token, intents);

        let media_downloader =
            Arc::new(MediaDownloader::new().context("Failed to initialize media downloader")?);

        if let Err(e) = media_downloader.test_setup().await {
            warn!("Media downloader test failed: {}", e);
        }

        let application_id = {
            let response = http.current_user_application().await?;
            response.model().await?.id
        };

        let user_id = {
            let response = http.current_user().await?;
            response.model().await?.id
        };

        let bot = Self {
            http: http.clone(),
            cache,
            media_downloader: media_downloader.clone(),
            config: Arc::new(config),
            application_id,
            user_id,
        };

        bot.register_commands().await?;

        Ok((bot, shard))
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
        .option(StringBuilder::new("message", "Message to send with the embed").required(false))
        .option(BooleanBuilder::new("spoiler", "Mark the embed as a spoiler").required(false))
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

    pub async fn run(self, mut shard: Shard) -> Result<()> {
        info!("Discord bot starting...");

        loop {
            let event = match shard
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
                    let bot = self.clone();
                    let msg = msg.clone();
                    tokio::spawn(async move {
                        if let Err(e) = bot.handle_message(&msg).await {
                            error!("Error handling message: {}", e);
                        }
                    });
                }
                Event::InteractionCreate(interaction) => {
                    let bot = self.clone();
                    let interaction = interaction.clone();
                    tokio::spawn(async move {
                        if let Err(e) = bot.handle_interaction(&interaction).await {
                            error!("Error handling interaction: {}", e);
                        }
                    });
                }
                Event::ReactionAdd(reaction) => {
                    let bot = self.clone();
                    let reaction = reaction.clone();
                    tokio::spawn(async move {
                        if let Err(e) = bot.handle_reaction_add(&reaction).await {
                            error!("Error handling reaction add: {}", e);
                        }
                    });
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
            let server_config = self.config.get_server_config(&guild_id.to_string());
            if server_config.is_auto_embed_channel(&msg.channel_id.to_string()) {
                for url in self.extract_urls(&msg.content) {
                    // Skip disabled domains silently
                    if server_config.is_domain_disabled(&url) {
                        info!("Skipping disabled domain in auto-embed channel: {}", url);
                        continue;
                    }

                    if self.media_downloader.is_supported_url(&url) {
                        match self.media_downloader.download(&url).await {
                            Ok(media_info) => {
                                info!("Downloaded media: {}", media_info.metadata.title);
                                if let Err(e) = self
                                    .send_media_to_channel(
                                        &msg.channel_id,
                                        Some(msg.author.id),
                                        &media_info,
                                        None,
                                        false,
                                    )
                                    .await
                                {
                                    let error_msg = format!("❌ Failed to send media: {}", e);
                                    let _ = self
                                        .http
                                        .create_message(msg.channel_id)
                                        .content(&error_msg)
                                        .await;
                                    error!("Failed to send media to channel: {}", e);
                                } else {
                                    let _ = self.http.delete_message(msg.channel_id, msg.id).await;
                                }
                            }
                            Err(e) => {
                                // Check if URL can be transformed (e.g., Instagram -> kkinstagram)
                                if let Some(transformed_url) =
                                    self.media_downloader.get_transformed_url(&url)
                                {
                                    info!(
                                        "Download failed, sending transformed URL: {}",
                                        transformed_url
                                    );
                                    let _ = self
                                        .http
                                        .create_message(msg.channel_id)
                                        .content(&format!(
                                            "<@{}> {}",
                                            msg.author.id, transformed_url
                                        ))
                                        .await;
                                    let _ = self.http.delete_message(msg.channel_id, msg.id).await;
                                } else {
                                    let cleaned_error = clean_error_message(&e);
                                    let error_msg =
                                        format!("Failed to download media: `{}`", cleaned_error);
                                    let _ = self
                                        .http
                                        .create_message(msg.channel_id)
                                        .content(&error_msg)
                                        .reply(msg.id)
                                        .await;
                                    error!("Failed to download media from {}: {}", url, e);
                                }
                            }
                        }
                        break; // Only process the first supported URL
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_reaction_add(&self, reaction: &ReactionAdd) -> Result<()> {
        // Only handle X emoji reactions
        match &reaction.emoji {
            EmojiReactionType::Unicode { name } if name == "❌" => {
                // Skip if the reactor is the bot itself
                if reaction.user_id == self.user_id {
                    return Ok(());
                }

                // Skip if the message was not posted by the bot user
                if reaction.message_author_id != Some(self.user_id) {
                    return Ok(());
                }

                // Check if the reaction was added by the message author
                if let Ok(message) = self
                    .http
                    .message(reaction.channel_id, reaction.message_id)
                    .await
                {
                    if let Ok(message_model) = message.model().await {
                        // Extract original user ID from message content (format: "shared by <@123456789>")
                        let original_user_id =
                            self.extract_original_user_from_content(&message_model.content);

                        if original_user_id == Some(reaction.user_id) {
                            // Delete the message
                            if let Err(e) = self
                                .http
                                .delete_message(reaction.channel_id, reaction.message_id)
                                .await
                            {
                                error!("Failed to delete message: {}", e);
                            }
                        }
                    }
                }
            }
            _ => {} // Ignore other emoji types
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
        let options = EmbedCommandOptions::from_command_data(data);

        if options.url.is_empty() {
            self.respond_to_interaction(interaction, "Please provide a valid URL.")
                .await?;
            return Ok(());
        }

        if !self.media_downloader.is_supported_url(&options.url) {
            self.respond_to_interaction(interaction, "This URL is not supported.")
                .await?;
            return Ok(());
        }

        // Acknowledge the interaction and download media concurrently
        let (ack_result, download_result) = join!(
            self.respond_to_interaction(interaction, "Downloading media..."),
            self.media_downloader.download(&options.url)
        );

        // Check if acknowledgment failed
        ack_result?;

        // Process the download result
        match download_result {
            Ok(media_info) => {
                info!("Successfully downloaded: {}", media_info.metadata.title);

                if !media_info.files.is_empty() {
                    // Use the working channel upload method instead of interaction followup
                    let channel_id = match interaction.channel.as_ref() {
                        Some(channel) => channel.id,
                        None => {
                            error!("No channel information in interaction");
                            let _ = self
                                .followup_message(
                                    interaction,
                                    "Cannot determine channel for upload",
                                )
                                .await;
                            return Ok(());
                        }
                    };

                    let user_id = interaction
                        .author_id()
                        .or_else(|| interaction.user.as_ref().map(|u| u.id));
                    if let Err(e) = self
                        .send_media_to_channel(
                            &channel_id,
                            user_id,
                            &media_info,
                            options.message,
                            options.spoiler,
                        )
                        .await
                    {
                        error!("Failed to send media to channel: {}", e);
                        let _ = self
                            .followup_message(interaction, "Failed to send media file")
                            .await;
                    }
                } else {
                    let _ = self
                        .followup_message(interaction, "Media processed but no files to send")
                        .await;
                }
            }
            Err(e) => {
                let cleaned_error = clean_error_message(&e);
                error!("Failed to download media from {}: {}", options.url, e);
                let _ = self
                    .followup_message(
                        interaction,
                        &format!(
                            "Failed to download media: {}\n{}",
                            cleaned_error, options.url
                        ),
                    )
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
                poll: None,
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
        user_id: Option<twilight_model::id::Id<twilight_model::id::marker::UserMarker>>,
        media_info: &crate::media::MediaInfo,
        message: Option<String>,
        spoiler: bool,
    ) -> Result<()> {
        if media_info.files.is_empty() {
            return Err(anyhow::anyhow!("No files to send"));
        }

        // Create attachments from in-memory files
        let mut attachments = Vec::new();
        let mut oversized_files = Vec::new();
        let mut attachment_id = 1u64;

        for file in &media_info.files {
            let file_size = file.data.len() as u64;

            debug!(
                "Processing file: {} (size: {} bytes)",
                file.filename, file_size
            );

            // Skip empty files
            if file_size == 0 {
                warn!("Skipping empty file: {}", file.filename);
                continue;
            }

            // Discord has a 10MB file size limit for most servers
            #[allow(unused_variables)]
            let (file_data, file_size) = if file_size > 10_000_000 {
                info!(
                    "File {} is too large ({} MB), attempting to resize",
                    file.filename,
                    file_size as f64 / 1_000_000.0
                );

                let is_video = file.filename.ends_with(".mp4")
                    || file.filename.ends_with(".webm")
                    || file.filename.ends_with(".mov");

                let resize_result = tokio::task::spawn_blocking({
                    let file_data = file.data.clone();
                    let file_name = file.filename.clone();
                    move || {
                        if is_video {
                            crate::media::resize_media_file(&file_data, &file_name, 10)
                        } else {
                            crate::media::resize_image_file(&file_data, &file_name, 10)
                        }
                    }
                })
                .await;

                match resize_result {
                    Ok(Ok(resized_data)) => {
                        info!(
                            "Successfully resized {} from {} to {} bytes",
                            file.filename,
                            file_size,
                            resized_data.len()
                        );
                        (resized_data.clone(), resized_data.len() as u64)
                    }
                    Ok(Err(e)) => {
                        warn!(
                            "Failed to resize {}: {}, marking as oversized",
                            file.filename, e
                        );
                        oversized_files.push((file.filename.clone(), file_size));
                        continue;
                    }
                    Err(e) => {
                        warn!("Resize task failed for {}: {}", file.filename, e);
                        oversized_files.push((file.filename.clone(), file_size));
                        continue;
                    }
                }
            } else {
                (file.data.clone(), file_size)
            };

            let file_name = if spoiler {
                format!("SPOILER_{}", file.filename)
            } else {
                file.filename.clone()
            };

            let attachment = Attachment::from_bytes(file_name, file_data, attachment_id);
            attachment_id += 1;

            attachments.push(attachment);
        }

        // If all files are oversized, send error message
        if attachments.is_empty() && !oversized_files.is_empty() {
            let oversized_list = oversized_files
                .iter()
                .map(|(name, size)| format!("{} ({:.1}MB)", name, *size as f64 / 1_000_000.0))
                .collect::<Vec<_>>()
                .join(", ");

            self.http
                .create_message(*channel_id)
                .content(&format!(
                    "❌ {} - All files too large. Discord limit is 10MB.\nOversized files: {}",
                    media_info.url, oversized_list
                ))
                .await?;
            return Ok(());
        }

        // Build message content with metadata
        let mut content = if let Some(user_id) = user_id {
            format!("<@{}>", user_id)
        } else {
            "".to_string()
        };

        content.push_str(&format!("\n{}", media_info.url));

        // Add author if available
        if let Some(author) = &media_info.metadata.author {
            content.push_str(&format!("\n👤 Author: {author}"));
        }

        // Add likes if available
        if let Some(likes) = media_info.metadata.likes {
            content.push_str(&format!(
                "\n❤️ Likes: {}",
                crate::utils::format_number(likes)
            ));
        }

        // Add title if available
        if !media_info.metadata.title.is_empty()
            && media_info.metadata.title != "Unknown Title"
            && media_info.metadata.title != "Unknown Media"
        {
            content.push_str(&format!("\n> {}", media_info.metadata.title,));
        }

        // Add user message if provided
        if let Some(message_content) = message {
            if !message_content.is_empty() {
                content.push_str(&format!("\n\n{message_content}"));
            }
        }

        // Add warning about oversized files if some were skipped
        if !oversized_files.is_empty() {
            let oversized_names = oversized_files
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            content.push_str(&format!("\nSkipped oversized files: {oversized_names}"));
        }

        // Send message with multiple attachments
        debug!("Sending message with {} attachments", attachments.len());
        debug!(
            "Attachment filenames: {:?}",
            attachments.iter().map(|a| &a.filename).collect::<Vec<_>>()
        );

        let message = self
            .http
            .create_message(*channel_id)
            .content(&content)
            .attachments(&attachments)
            .flags(MessageFlags::SUPPRESS_EMBEDS)
            .await?;

        // Add X reaction for easy deletion
        if let Ok(msg) = message.model().await {
            let _ = self
                .http
                .create_reaction(
                    msg.channel_id,
                    msg.id,
                    &RequestReactionType::Unicode { name: "❌" },
                )
                .await;
        }

        Ok(())
    }

    fn extract_original_user_from_content(
        &self,
        content: &str,
    ) -> Option<twilight_model::id::Id<twilight_model::id::marker::UserMarker>> {
        if let Some(start) = content.find("<@") {
            let user_mention = &content[start + 2..];
            if let Some(end) = user_mention.find('>') {
                let user_id_str = &user_mention[..end];
                if let Ok(user_id) = user_id_str.parse::<u64>() {
                    return Some(twilight_model::id::Id::new(user_id));
                }
            }
        }
        None
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

    let (bot, shard) = DiscordBot::new(token).await?;
    bot.run(shard).await
}

pub async fn run_with_config(config: ConfigManager) -> Result<()> {
    let token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN environment variable is required");

    let (bot, shard) = DiscordBot::new_with_config(token, config).await?;
    bot.run(shard).await
}

struct EmbedCommandOptions {
    url: String,
    message: Option<String>,
    spoiler: bool,
}

impl EmbedCommandOptions {
    fn from_command_data(data: &CommandData) -> Self {
        let mut url = String::new();
        let mut message = None;
        let mut spoiler = false;

        for opt in &data.options {
            match opt.name.as_str() {
                "url" => {
                    if let twilight_model::application::interaction::application_command::CommandOptionValue::String(s) = &opt.value {
                        url = s.clone();
                    }
                }
                "message" => {
                    if let twilight_model::application::interaction::application_command::CommandOptionValue::String(s) = &opt.value {
                        message = Some(s.clone());
                    }
                }
                "spoiler" => {
                    if let twilight_model::application::interaction::application_command::CommandOptionValue::Boolean(b) = &opt.value {
                        spoiler = *b;
                    }
                }
                _ => {}
            }
        }

        Self {
            url,
            message,
            spoiler,
        }
    }
}
