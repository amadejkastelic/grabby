use crate::{config::ConfigManager, media::MediaDownloader};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
    channel::message::{EmojiReactionType, MessageFlags, MessageReference, MessageReferenceType},
    gateway::payload::incoming::{MessageCreate, ReactionAdd},
    http::{
        attachment::Attachment,
        interaction::{InteractionResponse, InteractionResponseType},
    },
    id::{
        marker::{ApplicationMarker, ChannelMarker, MessageMarker, UserMarker},
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

const REFRESH_EMOJI: &str = "🔄";
const REFRESH_STATE_TTL: Duration = Duration::from_secs(3600);
const REFRESH_STATE_SWEEP_INTERVAL: Duration = Duration::from_secs(600);

#[derive(Clone)]
struct RefreshState {
    source_url: String,
    current_host: Option<String>,
    is_download: bool,
    spoiler: bool,
    created_at: Instant,
}

#[derive(Clone)]
pub struct DiscordBot {
    http: Arc<HttpClient>,
    cache: Arc<InMemoryCache>,
    media_downloader: Arc<MediaDownloader>,
    config: Arc<ConfigManager>,
    application_id: Id<ApplicationMarker>,
    user_id: Id<UserMarker>,
    refresh_state: Arc<Mutex<HashMap<Id<MessageMarker>, RefreshState>>>,
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
            warn!(error = %e, "Media downloader test failed");
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
            refresh_state: Arc::new(Mutex::new(HashMap::new())),
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

    fn spawn_refresh_state_sweeper(&self) {
        let bot = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(REFRESH_STATE_SWEEP_INTERVAL);
            interval.tick().await;
            loop {
                interval.tick().await;
                let now = Instant::now();
                let mut state = match bot.refresh_state.lock() {
                    Ok(state) => state,
                    Err(_) => continue,
                };
                let before = state.len();
                state.retain(|_, entry| now.duration_since(entry.created_at) < REFRESH_STATE_TTL);
                let swept = before - state.len();
                if swept > 0 {
                    debug!(
                        swept = swept,
                        remaining = state.len(),
                        "Swept expired refresh state entries"
                    );
                }
            }
        });
    }

    pub async fn run(self, mut shard: Shard) -> Result<()> {
        info!("Discord bot starting...");

        self.spawn_refresh_state_sweeper();

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
                            error!(error = %e, "Error handling message");
                        }
                    });
                }
                Event::InteractionCreate(interaction) => {
                    let bot = self.clone();
                    let interaction = interaction.clone();
                    tokio::spawn(async move {
                        if let Err(e) = bot.handle_interaction(&interaction).await {
                            error!(error = %e, "Error handling interaction");
                        }
                    });
                }
                Event::ReactionAdd(reaction) => {
                    let bot = self.clone();
                    let reaction = reaction.clone();
                    tokio::spawn(async move {
                        if let Err(e) = bot.handle_reaction_add(&reaction).await {
                            error!(error = %e, "Error handling reaction add");
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

        let reply_to = reply_target(msg.reference.as_ref());

        // Check if this is an auto-embed channel
        if let Some(guild_id) = msg.guild_id {
            let server_config = self.config.get_server_config(&guild_id.to_string());
            if server_config.is_auto_embed_channel(&msg.channel_id.to_string()) {
                for url in self.extract_urls(&msg.content) {
                    // Skip disabled domains silently (blacklist wins)
                    if server_config.is_domain_disabled(&url) {
                        info!(
                            user_id = msg.author.id.get(),
                            channel_id = msg.channel_id.get(),
                            guild_id = msg.guild_id.map(|g| g.get()),
                            url = %url,
                            "Skipping disabled domain in auto-embed channel"
                        );
                        continue;
                    }

                    // Skip non-whitelisted domains silently when a whitelist is configured
                    if !server_config.is_domain_allowed(&url) {
                        info!(
                            user_id = msg.author.id.get(),
                            channel_id = msg.channel_id.get(),
                            guild_id = msg.guild_id.map(|g| g.get()),
                            url = %url,
                            "Skipping non-whitelisted domain in auto-embed channel"
                        );
                        continue;
                    }

                    if server_config.transform_only {
                        if let Some(transformed_url) =
                            self.media_downloader.get_transformed_url(&url)
                        {
                            info!(
                                user_id = msg.author.id.get(),
                                channel_id = msg.channel_id.get(),
                                guild_id = msg.guild_id.map(|g| g.get()),
                                url = %url,
                                transformed_url = %transformed_url,
                                "Posting transformed URL (transform-only mode)"
                            );
                            let _ = self
                                .post_mirror_link(
                                    msg.channel_id,
                                    Some(msg.author.id),
                                    &url,
                                    &transformed_url,
                                    reply_to,
                                )
                                .await;
                            let _ = self.http.delete_message(msg.channel_id, msg.id).await;
                        } else {
                            info!(
                                user_id = msg.author.id.get(),
                                channel_id = msg.channel_id.get(),
                                guild_id = msg.guild_id.map(|g| g.get()),
                                url = %url,
                                "Skipping URL with no transform in transform-only mode"
                            );
                        }
                        break;
                    }

                    if self.media_downloader.is_supported_url(&url) {
                        match self.media_downloader.download(&url).await {
                            Ok(media_info) => {
                                info!(
                                    user_id = msg.author.id.get(),
                                    channel_id = msg.channel_id.get(),
                                    guild_id = msg.guild_id.map(|g| g.get()),
                                    url = %url,
                                    title = %media_info.metadata.title,
                                    file_count = media_info.files.len(),
                                    "Downloaded media"
                                );
                                if let Err(e) = self
                                    .send_media_to_channel(
                                        &msg.channel_id,
                                        Some(msg.author.id),
                                        &media_info,
                                        None,
                                        false,
                                        reply_to,
                                    )
                                    .await
                                {
                                    let error_msg = format!("❌ Failed to send media: {}", e);
                                    let _ = self
                                        .http
                                        .create_message(msg.channel_id)
                                        .content(&error_msg)
                                        .await;
                                    error!(
                                        user_id = msg.author.id.get(),
                                        channel_id = msg.channel_id.get(),
                                        message_id = msg.id.get(),
                                        url = %url,
                                        error = %e,
                                        "Failed to send media to channel"
                                    );
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
                                        user_id = msg.author.id.get(),
                                        channel_id = msg.channel_id.get(),
                                        guild_id = msg.guild_id.map(|g| g.get()),
                                        url = %url,
                                        transformed_url = %transformed_url,
                                        "Download failed, sending transformed URL"
                                    );
                                    let _ = self
                                        .post_mirror_link(
                                            msg.channel_id,
                                            Some(msg.author.id),
                                            &url,
                                            &transformed_url,
                                            reply_to,
                                        )
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
                                    error!(
                                        user_id = msg.author.id.get(),
                                        channel_id = msg.channel_id.get(),
                                        guild_id = msg.guild_id.map(|g| g.get()),
                                        url = %url,
                                        error = %e,
                                        "Failed to download media"
                                    );
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
                                error!(
                                    reactor_id = reaction.user_id.get(),
                                    channel_id = reaction.channel_id.get(),
                                    message_id = reaction.message_id.get(),
                                    original_author_id = original_user_id.map(|id| id.get()),
                                    error = %e,
                                    "Failed to delete message via reaction"
                                );
                            }
                        }
                    }
                }
            }
            EmojiReactionType::Unicode { name } if name == REFRESH_EMOJI => {
                self.handle_refresh_reaction(reaction).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn fetch_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Option<twilight_model::channel::Message> {
        let response = self.http.message(channel_id, message_id).await.ok()?;
        response.model().await.ok()
    }

    fn restore_refresh_state(&self, message_id: Id<MessageMarker>, entry: RefreshState) {
        if let Ok(mut state) = self.refresh_state.lock() {
            state.entry(message_id).or_insert(entry);
        }
    }

    async fn handle_refresh_reaction(&self, reaction: &ReactionAdd) -> Result<()> {
        if reaction.user_id == self.user_id {
            return Ok(());
        }
        if reaction.message_author_id != Some(self.user_id) {
            return Ok(());
        }

        let Some(message) = self
            .fetch_message(reaction.channel_id, reaction.message_id)
            .await
        else {
            return Ok(());
        };

        let original_user_id = self.extract_original_user_from_content(&message.content);
        if original_user_id != Some(reaction.user_id) {
            return Ok(());
        }

        let entry = match self.refresh_state.lock() {
            Ok(mut state) => state.remove(&reaction.message_id),
            Err(_) => return Ok(()),
        };
        let Some(entry) = entry else {
            return Ok(());
        };

        let Some((parsed_source, mirrors)) = crate::media::get_mirrors(&entry.source_url) else {
            self.restore_refresh_state(reaction.message_id, entry);
            return Ok(());
        };
        if mirrors.len() < 2 {
            self.restore_refresh_state(reaction.message_id, entry);
            return Ok(());
        }

        let start_index = pick_next_index(mirrors, entry.current_host.as_deref());

        let new_host = if entry.is_download {
            self.refresh_download(
                reaction,
                original_user_id,
                &entry,
                &parsed_source,
                mirrors,
                start_index,
            )
            .await?
        } else {
            self.refresh_transform(
                reaction,
                original_user_id,
                &parsed_source,
                mirrors,
                start_index,
            )
            .await?
        };

        if let Some(new_host) = new_host {
            if let Ok(mut state) = self.refresh_state.lock() {
                state.insert(
                    reaction.message_id,
                    RefreshState {
                        source_url: entry.source_url,
                        current_host: Some(new_host),
                        is_download: entry.is_download,
                        spoiler: entry.spoiler,
                        created_at: Instant::now(),
                    },
                );
            }
        } else {
            self.restore_refresh_state(reaction.message_id, entry);
        }

        Ok(())
    }

    async fn refresh_download(
        &self,
        reaction: &ReactionAdd,
        original_user_id: Option<Id<UserMarker>>,
        entry: &RefreshState,
        parsed_source: &url::Url,
        mirrors: &[&str],
        start_index: usize,
    ) -> Result<Option<String>> {
        let n = mirrors.len();
        for offset in 0..n {
            let idx = (start_index + offset) % n;
            let mirror_host = mirrors[idx];
            let Some(mirror_url) = crate::media::transform_to_host(parsed_source, mirror_host)
            else {
                continue;
            };

            match self.media_downloader.download(&mirror_url).await {
                Ok(media_info) => {
                    let (attachments, oversized) =
                        Self::build_attachments(&media_info, entry.spoiler).await;
                    if attachments.is_empty() {
                        warn!(
                            mirror_host,
                            "Refresh mirror returned no usable attachments; trying next"
                        );
                        continue;
                    }

                    let content = Self::build_media_content(
                        original_user_id,
                        &entry.source_url,
                        &media_info,
                        None,
                        &oversized,
                    );

                    if let Err(e) = self
                        .http
                        .update_message(reaction.channel_id, reaction.message_id)
                        .content(Some(&content))
                        .attachments(&attachments)
                        .keep_attachment_ids(&[])
                        .flags(MessageFlags::SUPPRESS_EMBEDS)
                        .await
                    {
                        warn!(mirror_host = %mirror_host, error = %e, "Failed to edit message during refresh");
                        continue;
                    }

                    info!(mirror_host = %mirror_host, "Refreshed media from new mirror");
                    return Ok(Some(mirror_host.to_string()));
                }
                Err(e) => {
                    warn!(mirror_host = %mirror_host, error = %e, "Refresh download from mirror failed");
                    continue;
                }
            }
        }

        warn!("All mirrors failed during refresh; leaving message unchanged");
        Ok(None)
    }

    async fn refresh_transform(
        &self,
        reaction: &ReactionAdd,
        original_user_id: Option<Id<UserMarker>>,
        parsed_source: &url::Url,
        mirrors: &[&str],
        start_index: usize,
    ) -> Result<Option<String>> {
        let mirror_host = mirrors[start_index];
        let Some(new_mirror_url) = crate::media::transform_to_host(parsed_source, mirror_host)
        else {
            return Ok(None);
        };
        let new_content = match original_user_id {
            Some(uid) => format!("<@{uid}> {new_mirror_url}"),
            None => new_mirror_url.clone(),
        };

        if let Err(e) = self
            .http
            .update_message(reaction.channel_id, reaction.message_id)
            .content(Some(&new_content))
            .await
        {
            warn!(mirror_host = %mirror_host, error = %e, "Failed to swap mirror link during refresh");
            return Ok(None);
        }

        info!(mirror_host = %mirror_host, "Swapped mirror link during refresh");
        Ok(Some(mirror_host.to_string()))
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
                            info!(
                                command = %data.name,
                                user_id = interaction.author_id().map(|id| id.get()),
                                channel_id = interaction.channel.as_ref().map(|c| c.id.get()),
                                guild_id = interaction.guild_id.map(|g| g.get()),
                                interaction_id = interaction.id.get(),
                                "Unknown command"
                            );
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

        // Transform-only mode: post the transformed URL instead of downloading
        let transform_only = interaction
            .guild_id
            .map(|gid| {
                self.config
                    .get_server_config(&gid.to_string())
                    .transform_only
            })
            .unwrap_or(false);

        if transform_only {
            if let Some(transformed_url) = self.media_downloader.get_transformed_url(&options.url) {
                self.respond_to_interaction(interaction, "Transforming URL...")
                    .await?;

                let channel_id = match interaction.channel.as_ref() {
                    Some(channel) => channel.id,
                    None => {
                        error!(
                            user_id = interaction.author_id().map(|id| id.get()),
                            guild_id = interaction.guild_id.map(|g| g.get()),
                            interaction_id = interaction.id.get(),
                            "No channel information in interaction"
                        );
                        let _ = self
                            .followup_message(interaction, "Cannot determine channel for upload")
                            .await;
                        return Ok(());
                    }
                };

                let user_id = interaction
                    .author_id()
                    .or_else(|| interaction.user.as_ref().map(|u| u.id));

                info!(
                    user_id = interaction.author_id().map(|id| id.get()),
                    channel_id = interaction.channel.as_ref().map(|c| c.id.get()),
                    guild_id = interaction.guild_id.map(|g| g.get()),
                    url = %options.url,
                    transformed_url = %transformed_url,
                    "Posting transformed URL (transform-only mode)"
                );

                if let Err(e) = self
                    .post_mirror_link(channel_id, user_id, &options.url, &transformed_url, None)
                    .await
                {
                    error!(
                        user_id = user_id.map(|id| id.get()),
                        channel_id = channel_id.get(),
                        url = %options.url,
                        error = %e,
                        "Failed to send transformed URL"
                    );
                    let _ = self
                        .followup_message(interaction, "Failed to send transformed URL")
                        .await;
                }
            } else {
                info!(
                    user_id = interaction.author_id().map(|id| id.get()),
                    channel_id = interaction.channel.as_ref().map(|c| c.id.get()),
                    guild_id = interaction.guild_id.map(|g| g.get()),
                    url = %options.url,
                    "Skipping /embed: no transform available in transform-only mode"
                );
                self.respond_to_interaction(
                    interaction,
                    "No transform available for this domain in transform-only mode.",
                )
                .await?;
            }
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
                info!(
                    user_id = interaction.author_id().map(|id| id.get()),
                    channel_id = interaction.channel.as_ref().map(|c| c.id.get()),
                    guild_id = interaction.guild_id.map(|g| g.get()),
                    url = %options.url,
                    title = %media_info.metadata.title,
                    file_count = media_info.files.len(),
                    "Successfully downloaded media"
                );

                if !media_info.files.is_empty() {
                    // Use the working channel upload method instead of interaction followup
                    let channel_id = match interaction.channel.as_ref() {
                        Some(channel) => channel.id,
                        None => {
                            error!(
                                user_id = interaction.author_id().map(|id| id.get()),
                                guild_id = interaction.guild_id.map(|g| g.get()),
                                interaction_id = interaction.id.get(),
                                "No channel information in interaction"
                            );
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
                            None,
                        )
                        .await
                    {
                        error!(
                            user_id = user_id.map(|id| id.get()),
                            channel_id = channel_id.get(),
                            url = %options.url,
                            error = %e,
                            "Failed to send media to channel"
                        );
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
                error!(
                    user_id = interaction.author_id().map(|id| id.get()),
                    channel_id = interaction.channel.as_ref().map(|c| c.id.get()),
                    guild_id = interaction.guild_id.map(|g| g.get()),
                    url = %options.url,
                    error = %e,
                    "Failed to download media"
                );
                let message = if let Some(transformed_url) =
                    self.media_downloader.get_transformed_url(&options.url)
                {
                    transformed_url
                } else {
                    options.url.clone()
                };
                let _ = self.followup_message(interaction, &message).await;
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

    async fn build_attachments(
        media_info: &crate::media::MediaInfo,
        spoiler: bool,
    ) -> (Vec<Attachment>, Vec<(String, u64)>) {
        let mut attachments = Vec::new();
        let mut oversized_files = Vec::new();
        let mut attachment_id = 1u64;

        for file in &media_info.files {
            let file_size = file.data.len() as u64;

            debug!(file_name = %file.filename, file_size = file_size, "Processing file");

            if file_size == 0 {
                warn!(file_name = %file.filename, "Skipping empty file");
                continue;
            }

            #[allow(unused_variables)]
            let (file_data, file_size) = if file_size > 10_000_000 {
                info!(
                    file_name = %file.filename,
                    file_size,
                    max_size_mb = 10u64,
                    "File exceeds size limit, attempting to resize"
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
                            file_name = %file.filename,
                            original_size = file_size,
                            new_size = resized_data.len() as u64,
                            "Successfully resized file"
                        );
                        (resized_data.clone(), resized_data.len() as u64)
                    }
                    Ok(Err(e)) => {
                        warn!(
                            file_name = %file.filename,
                            error = %e,
                            "Failed to resize file, marking as oversized"
                        );
                        oversized_files.push((file.filename.clone(), file_size));
                        continue;
                    }
                    Err(e) => {
                        warn!(file_name = %file.filename, error = %e, "Resize task failed");
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

        (attachments, oversized_files)
    }

    fn build_media_content(
        user_id: Option<Id<UserMarker>>,
        url: &str,
        media_info: &crate::media::MediaInfo,
        message: Option<&str>,
        oversized_files: &[(String, u64)],
    ) -> String {
        let mut content = match user_id {
            Some(uid) => format!("<@{uid}>"),
            None => String::new(),
        };

        content.push_str(&format!("\n{url}"));

        if let Some(author) = &media_info.metadata.author {
            content.push_str(&format!("\n👤 Author: {author}"));
        }

        if let Some(likes) = media_info.metadata.likes {
            content.push_str(&format!(
                "\n❤️ Likes: {}",
                crate::utils::format_number(likes)
            ));
        }

        if !media_info.metadata.title.is_empty()
            && media_info.metadata.title != "Unknown Title"
            && media_info.metadata.title != "Unknown Media"
        {
            content.push_str(&format!("\n> {}", media_info.metadata.title));
        }

        if let Some(message_content) = message {
            if !message_content.is_empty() {
                content.push_str(&format!("\n\n{message_content}"));
            }
        }

        if !oversized_files.is_empty() {
            let oversized_names = oversized_files
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            content.push_str(&format!("\nSkipped oversized files: {oversized_names}"));
        }

        content
    }

    async fn send_media_to_channel(
        &self,
        channel_id: &Id<ChannelMarker>,
        user_id: Option<twilight_model::id::Id<twilight_model::id::marker::UserMarker>>,
        media_info: &crate::media::MediaInfo,
        message: Option<String>,
        spoiler: bool,
        reply_to: Option<Id<MessageMarker>>,
    ) -> Result<()> {
        if media_info.files.is_empty() {
            return Err(anyhow::anyhow!("No files to send"));
        }

        let (attachments, oversized_files) = Self::build_attachments(media_info, spoiler).await;

        if attachments.is_empty() && !oversized_files.is_empty() {
            let url = self
                .media_downloader
                .get_transformed_url(&media_info.url)
                .unwrap_or_else(|| media_info.url.clone());
            let mut request = self.http.create_message(*channel_id).content(&url);
            if let Some(reply_to) = reply_to {
                request = request.reply(reply_to).fail_if_not_exists(false);
            }
            request.await?;
            return Ok(());
        }

        let content = Self::build_media_content(
            user_id,
            &media_info.url,
            media_info,
            message.as_deref(),
            &oversized_files,
        );

        debug!(
            channel_id = channel_id.get(),
            attachment_count = attachments.len(),
            "Sending message with attachments"
        );
        debug!(
            channel_id = channel_id.get(),
            filenames = ?attachments.iter().map(|a| &a.filename).collect::<Vec<_>>(),
            "Attachment filenames"
        );

        let mut request = self
            .http
            .create_message(*channel_id)
            .content(&content)
            .attachments(&attachments)
            .flags(MessageFlags::SUPPRESS_EMBEDS);

        if let Some(reply_to) = reply_to {
            request = request.reply(reply_to).fail_if_not_exists(false);
        }

        let message = request.await?;

        if let Ok(msg) = message.model().await {
            let _ = self
                .http
                .create_reaction(
                    msg.channel_id,
                    msg.id,
                    &RequestReactionType::Unicode { name: "❌" },
                )
                .await;

            if crate::media::get_mirrors(&media_info.url)
                .is_some_and(|(_, mirrors)| mirrors.len() >= 2)
            {
                let _ = self
                    .http
                    .create_reaction(
                        msg.channel_id,
                        msg.id,
                        &RequestReactionType::Unicode {
                            name: REFRESH_EMOJI,
                        },
                    )
                    .await;

                let state = RefreshState {
                    source_url: media_info.url.clone(),
                    current_host: None,
                    is_download: true,
                    spoiler,
                    created_at: Instant::now(),
                };
                if let Ok(mut state_map) = self.refresh_state.lock() {
                    state_map.insert(msg.id, state);
                }
            }
        }

        Ok(())
    }

    async fn post_mirror_link(
        &self,
        channel_id: Id<ChannelMarker>,
        user_id: Option<Id<UserMarker>>,
        source_url: &str,
        mirror_url: &str,
        reply_to: Option<Id<MessageMarker>>,
    ) -> Result<()> {
        let content = match user_id {
            Some(uid) => format!("<@{uid}> {mirror_url}"),
            None => mirror_url.to_string(),
        };

        let mut request = self.http.create_message(channel_id).content(&content);

        if let Some(reply_to) = reply_to {
            request = request.reply(reply_to).fail_if_not_exists(false);
        }

        let message = request.await?;

        if let Ok(msg) = message.model().await {
            let _ = self
                .http
                .create_reaction(
                    msg.channel_id,
                    msg.id,
                    &RequestReactionType::Unicode { name: "❌" },
                )
                .await;

            if let Some((_, mirrors)) =
                crate::media::get_mirrors(source_url).filter(|(_, mirrors)| mirrors.len() >= 2)
            {
                let _ = self
                    .http
                    .create_reaction(
                        msg.channel_id,
                        msg.id,
                        &RequestReactionType::Unicode {
                            name: REFRESH_EMOJI,
                        },
                    )
                    .await;

                let first_host = mirrors.first().copied().map(String::from);
                let state = RefreshState {
                    source_url: source_url.to_string(),
                    current_host: first_host,
                    is_download: false,
                    spoiler: false,
                    created_at: Instant::now(),
                };
                if let Ok(mut state_map) = self.refresh_state.lock() {
                    state_map.insert(msg.id, state);
                }
            }
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

fn pick_next_index(mirrors: &[&str], current_host: Option<&str>) -> usize {
    match current_host {
        Some(host) => mirrors
            .iter()
            .position(|m| *m == host)
            .map(|pos| (pos + 1) % mirrors.len())
            .unwrap_or(0),
        None => 0,
    }
}

fn reply_target(reference: Option<&MessageReference>) -> Option<Id<MessageMarker>> {
    reference
        .filter(|reference| matches!(reference.kind, MessageReferenceType::Default))
        .and_then(|reference| reference.message_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(
        kind: MessageReferenceType,
        message_id: Option<Id<MessageMarker>>,
    ) -> MessageReference {
        MessageReference {
            channel_id: None,
            guild_id: None,
            kind,
            message_id,
            fail_if_not_exists: None,
        }
    }

    #[test]
    fn reply_target_returns_replied_to_message() {
        let reply = reference(MessageReferenceType::Default, Some(Id::new(1234)));

        assert_eq!(reply_target(Some(&reply)), Some(Id::new(1234)));
    }

    #[test]
    fn reply_target_ignores_forwards_and_missing_targets() {
        let forward = reference(MessageReferenceType::Forward, Some(Id::new(1234)));
        let no_target = reference(MessageReferenceType::Default, None);

        assert_eq!(reply_target(Some(&forward)), None);
        assert_eq!(reply_target(Some(&no_target)), None);
        assert_eq!(reply_target(None), None);
    }
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
