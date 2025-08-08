use super::{
    downloader::Downloader,
    types::{MediaFile, MediaInfo, MediaMetadata},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, info, warn};

pub struct YtDlpDownloader;

impl YtDlpDownloader {
    pub fn new() -> Self {
        Self
    }

    async fn extract_metadata(&self, url: &str) -> Result<MediaMetadata> {
        debug!("Extracting metadata with yt-dlp for: {}", url);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("yt-dlp")
                .arg("--dump-json")
                .arg("--no-download")
                .arg("--no-warnings")
                .arg(url)
                .output(),
        )
        .await
        .context("Media metadata extraction timed out")?
        .context("Failed to extract media metadata")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Media metadata extraction failed: {}",
                error
            ));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let json: Value =
            serde_json::from_str(&json_str).context("Failed to parse media metadata")?;

        debug!("yt-dlp JSON output: {}", json_str);

        Ok(MediaMetadata {
            title: json["title"]
                .as_str()
                .unwrap_or("Unknown Title")
                .to_string(),
            id: json["id"].as_str().unwrap_or("video").to_string(),
            thumbnail: json["thumbnail"].as_str().map(|s| s.to_string()),
            duration: json["duration"].as_f64().map(|d| d as u64),
            author: json["uploader"].as_str().map(|s| s.to_string()),
            likes: json["like_count"].as_u64(),
        })
    }

    async fn download_to_memory(
        &self,
        url: &str,
        metadata: &MediaMetadata,
    ) -> Result<Vec<MediaFile>> {
        info!("Downloading media with yt-dlp: {}", metadata.id);

        // Use yt-dlp to output to stdout
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(120), // 2 minutes for download
            tokio::process::Command::new("yt-dlp")
                .arg("--output")
                .arg("-") // Output to stdout
                .arg("--format")
                .arg("best[height<=720]/bestvideo[height<=720]+bestaudio/best[filesize<25M]/bestvideo+bestaudio/best")
                .arg("--no-warnings")
                .arg(url)
                .output()
        )
        .await
        .context("Media download timed out")?
        .context("Failed to download media")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);

            // If format issue, try without format specification
            if error.contains("Requested format is not available") {
                warn!("Format not available, retrying without format specification...");

                let retry_output = tokio::time::timeout(
                    std::time::Duration::from_secs(120), // 2 minutes for retry
                    tokio::process::Command::new("yt-dlp")
                        .arg("--output")
                        .arg("-")
                        .arg("--no-warnings")
                        .arg(url)
                        .output(),
                )
                .await
                .context("Media download retry timed out")?
                .context("Failed to retry media download")?;

                if !retry_output.status.success() {
                    let retry_error = String::from_utf8_lossy(&retry_output.stderr);
                    return Err(anyhow::anyhow!("Media download failed: {}", retry_error));
                }

                return Ok(vec![MediaFile {
                    filename: format!("{}.mp4", metadata.id),
                    data: retry_output.stdout,
                    content_type: Some("video/mp4".to_string()),
                }]);
            } else {
                return Err(anyhow::anyhow!("Media download failed: {}", error));
            }
        }

        Ok(vec![MediaFile {
            filename: format!("{}.mp4", metadata.id),
            data: output.stdout,
            content_type: Some("video/mp4".to_string()),
        }])
    }
}

#[async_trait]
impl Downloader for YtDlpDownloader {
    fn name(&self) -> &'static str {
        "yt-dlp"
    }

    async fn download(&self, url: &str) -> Result<MediaInfo> {
        let metadata = self.extract_metadata(url).await?;
        let files = self.download_to_memory(url, &metadata).await?;

        Ok(MediaInfo {
            url: url.to_string(),
            files,
            metadata,
        })
    }

    async fn test_availability() -> bool {
        match tokio::process::Command::new("yt-dlp")
            .arg("--version")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout);
                    info!("✅ yt-dlp is available, version: {}", version.trim());
                    true
                } else {
                    warn!("❌ yt-dlp command failed");
                    false
                }
            }
            Err(e) => {
                warn!("❌ yt-dlp not found: {}", e);
                false
            }
        }
    }
}
