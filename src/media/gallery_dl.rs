use super::{
    downloader::Downloader,
    types::{MediaFile, MediaInfo, MediaMetadata},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, info, warn};

pub struct GalleryDlDownloader;

impl GalleryDlDownloader {
    pub fn new() -> Self {
        Self
    }

    async fn extract_metadata_and_urls(&self, url: &str) -> Result<(MediaMetadata, Vec<String>)> {
        debug!("Extracting metadata with gallery-dl for: {}", url);

        let output = tokio::process::Command::new("gallery-dl")
            .arg("--dump-json")
            .arg(url)
            .output()
            .await
            .context("Failed to execute gallery-dl - make sure it's installed")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "gallery-dl metadata extraction failed: {}",
                error
            ));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        debug!("gallery-dl raw JSON output: {}", json_str);

        // gallery-dl outputs a JSON array
        let json_array: Value =
            serde_json::from_str(&json_str).context("Failed to parse gallery-dl JSON output")?;

        // Check if it's an empty array (no media found)
        let array = json_array
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("gallery-dl output is not a JSON array"))?;

        if array.is_empty() {
            return Err(anyhow::anyhow!("No media found by gallery-dl for this URL"));
        }

        // Gallery-dl format: [[type, metadata], [type, url, metadata], ...]
        // Find all media items with URLs and extract metadata
        let mut urls = Vec::new();
        let mut metadata = None;

        for item in array {
            if let Some(item_array) = item.as_array() {
                // Look for items with 3 elements: [type, url, metadata_object]
                if item_array.len() == 3 {
                    if let (Some(url_val), Some(meta)) = (item_array.get(1), item_array.get(2)) {
                        if let Some(url_str) = url_val.as_str() {
                            urls.push(url_str.to_string());

                            // Use the first metadata we find
                            if metadata.is_none() {
                                debug!("gallery-dl media item metadata: {}", meta);
                                metadata = Some(MediaMetadata {
                                    title: meta["content"]
                                        .as_str()
                                        .or(meta["filename"].as_str())
                                        .or(meta["title"].as_str())
                                        .unwrap_or("Unknown Media")
                                        .to_string(),
                                    thumbnail: None, // We'll have the actual images
                                    duration: None,  // Images don't have duration
                                    author: meta["author"]["name"]
                                        .as_str()
                                        .or(meta["author"]["nick"].as_str())
                                        .or(meta["uploader"].as_str())
                                        .map(|s| s.to_string()),
                                    likes: meta["favorite_count"].as_u64(),
                                });
                            }
                        }
                    }
                }
            }
        }

        let metadata = metadata
            .ok_or_else(|| anyhow::anyhow!("No media metadata found in gallery-dl output"))?;

        if urls.is_empty() {
            return Err(anyhow::anyhow!("No media URLs found in gallery-dl output"));
        }

        debug!("Found {} media URLs", urls.len());
        Ok((metadata, urls))
    }

    async fn download_url_to_memory(&self, url: &str, index: usize) -> Result<MediaFile> {
        debug!("Downloading URL to memory: {}", url);

        // Download the URL content
        let response = reqwest::get(url)
            .await
            .context("Failed to fetch media URL")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download media: HTTP {}",
                response.status()
            ));
        }

        // Get content type and data
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|ct| ct.to_str().ok())
            .map(|s| s.to_string());

        let data = response
            .bytes()
            .await
            .context("Failed to read media data")?
            .to_vec();

        // Generate filename based on content type or URL
        let extension = if let Some(ct) = &content_type {
            match ct.as_str() {
                s if s.starts_with("image/jpeg") => "jpg",
                s if s.starts_with("image/png") => "png",
                s if s.starts_with("image/gif") => "gif",
                s if s.starts_with("image/webp") => "webp",
                s if s.starts_with("video/mp4") => "mp4",
                s if s.starts_with("video/webm") => "webm",
                _ => "bin",
            }
        } else if url.ends_with(".jpg") || url.ends_with(".jpeg") {
            "jpg"
        } else if url.ends_with(".png") {
            "png"
        } else if url.ends_with(".gif") {
            "gif"
        } else if url.ends_with(".webp") {
            "webp"
        } else if url.ends_with(".mp4") {
            "mp4"
        } else {
            "bin"
        };

        let filename = if index == 0 {
            format!("image.{extension}")
        } else {
            format!("image_{}.{extension}", index + 1)
        };

        Ok(MediaFile {
            filename,
            data,
            content_type,
        })
    }
}

#[async_trait]
impl Downloader for GalleryDlDownloader {
    fn name(&self) -> &'static str {
        "gallery-dl"
    }

    async fn download(&self, url: &str) -> Result<MediaInfo> {
        let (metadata, media_urls) = self.extract_metadata_and_urls(url).await?;

        info!(
            "Downloading {} media files with gallery-dl: {}",
            media_urls.len(),
            metadata.title
        );

        // Download all media URLs to memory
        let mut files = Vec::new();
        for (index, media_url) in media_urls.iter().enumerate() {
            match self.download_url_to_memory(media_url, index).await {
                Ok(file) => files.push(file),
                Err(e) => warn!("Failed to download {}: {}", media_url, e),
            }
        }

        if files.is_empty() {
            return Err(anyhow::anyhow!("Failed to download any media files"));
        }

        Ok(MediaInfo {
            url: url.to_string(),
            files,
            metadata,
        })
    }

    async fn test_availability() -> bool {
        match tokio::process::Command::new("gallery-dl")
            .arg("--version")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout);
                    info!("✅ gallery-dl is available, version: {}", version.trim());
                    true
                } else {
                    warn!("❌ gallery-dl command failed");
                    false
                }
            }
            Err(e) => {
                warn!("❌ gallery-dl not found: {}", e);
                false
            }
        }
    }
}
