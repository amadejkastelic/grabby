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
            format_ext: json["ext"].as_str().unwrap_or("mp4").to_string(),
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
                .arg("best[height<=720]/best")
                .arg("--merge-output-format")
                .arg("mp4")
                .arg("--recode-video")
                .arg("mp4")
                .arg("--postprocessor-args")
                .arg("ffmpeg:-fs 7M")
                .arg("--no-warnings")
                .arg(url)
                .output(),
        )
        .await
        .context("Media download timed out")?
        .context("Failed to download media")?;

        let filename = format!("{}.{}", metadata.id, metadata.format_ext,);

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
                        .arg("--merge-output-format")
                        .arg("mp4")
                        .arg("--recode-video")
                        .arg("mp4")
                        .arg("--postprocessor-args")
                        .arg("ffmpeg:-fs 7M")
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
                    filename,
                    data: retry_output.stdout,
                }]);
            } else {
                return Err(anyhow::anyhow!("Media download failed: {}", error));
            }
        }

        Ok(vec![MediaFile {
            filename,
            data: output.stdout,
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
        // Test yt-dlp
        let yt_dlp_available = match tokio::process::Command::new("yt-dlp")
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
        };

        // Test ffmpeg (required for merging and re-encoding)
        let ffmpeg_available = match tokio::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let version_line = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .unwrap_or("unknown")
                        .to_string();
                    info!("✅ ffmpeg is available: {}", version_line);
                    true
                } else {
                    warn!("❌ ffmpeg command failed");
                    false
                }
            }
            Err(e) => {
                warn!(
                    "❌ ffmpeg not found: {} (required for video merging/re-encoding)",
                    e
                );
                false
            }
        };

        if yt_dlp_available && !ffmpeg_available {
            warn!("⚠️  yt-dlp will work but video merging/re-encoding features will be disabled");
        }

        yt_dlp_available
    }
}
