use super::{
    downloader::Downloader,
    types::{MediaFile, MediaInfo, MediaMetadata},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, info, warn};

pub struct YtDlpDownloader;

impl YtDlpDownloader {
    pub fn new() -> Self {
        Self
    }

    /// Parses yt-dlp JSON output and extracts metadata.
    fn parse_json(json_value: &Value) -> Result<MediaMetadata> {
        Ok(MediaMetadata {
            title: extract_title(json_value),
            id: extract_id(json_value),
            thumbnail: extract_thumbnail(json_value),
            duration: extract_duration(json_value),
            author: extract_author(json_value),
            likes: extract_likes(json_value),
            format_ext: extract_extension(json_value),
        })
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

        Self::parse_json(&json)
    }

    async fn download_to_memory(
        &self,
        url: &str,
        metadata: &MediaMetadata,
    ) -> Result<Vec<MediaFile>> {
        info!("Downloading media with yt-dlp: {}", metadata.id);

        // Force H.264 codec for Discord Linux compatibility
        // Try H.264 formats first (vcodec=h264 or starts with h264), fallback to best available
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            Command::new("yt-dlp")
                .arg("--output")
                .arg("-")
                .arg("--format")
                .arg("bestvideo[vcodec=h264]+bestaudio/best[vcodec=h264]/bestvideo[vcodec=avc1]+bestaudio/best[vcodec=avc1]/best")
                .arg("--merge-output-format")
                .arg("mp4")
                .arg("--no-warnings")
                .arg(url)
                .output(),
        )
        .await
        .context("Media download timed out")?
        .context("Failed to download media")?;

        let filename = format!("{}.{}", metadata.id, metadata.format_ext);

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Media download failed: {}", error));
        }

        Ok(vec![MediaFile {
            filename,
            data: output.stdout,
        }])
    }
}

fn extract_title(json: &Value) -> String {
    json["title"]
        .as_str()
        .unwrap_or("Unknown Title")
        .to_string()
}

fn extract_id(json: &Value) -> String {
    json["id"].as_str().unwrap_or("video").to_string()
}

fn extract_thumbnail(json: &Value) -> Option<String> {
    json["thumbnail"].as_str().map(|s| s.to_string())
}

fn extract_duration(json: &Value) -> Option<u64> {
    json["duration"].as_f64().map(|d| d as u64)
}

fn extract_author(json: &Value) -> Option<String> {
    json["uploader"].as_str().map(|s| s.to_string())
}

fn extract_likes(json: &Value) -> Option<u64> {
    json["like_count"].as_u64()
}

fn extract_extension(json: &Value) -> String {
    json["ext"].as_str().unwrap_or("mp4").to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn youtube_video_json() -> Value {
        serde_json::json!({
            "id": "dQw4w9WgXcQ",
            "title": "Never Gonna Give You Up",
            "uploader": "Rick Astley",
            "thumbnail": "https://example.com/thumb.jpg",
            "duration": 212.0,
            "like_count": 15000000,
            "ext": "mp4"
        })
    }

    #[test]
    fn test_parse_youtube_video_json() {
        let json = youtube_video_json();
        let result = YtDlpDownloader::parse_json(&json);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.id, "dQw4w9WgXcQ");
        assert_eq!(metadata.title, "Never Gonna Give You Up");
        assert_eq!(metadata.author, Some("Rick Astley".to_string()));
        assert_eq!(
            metadata.thumbnail,
            Some("https://example.com/thumb.jpg".to_string())
        );
        assert_eq!(metadata.duration, Some(212));
        assert_eq!(metadata.likes, Some(15000000));
        assert_eq!(metadata.format_ext, "mp4");
    }

    #[test]
    fn test_parse_minimal_json() {
        let json = serde_json::json!({
            "id": "test123"
        });
        let result = YtDlpDownloader::parse_json(&json);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.id, "test123");
        assert_eq!(metadata.title, "Unknown Title");
        assert_eq!(metadata.thumbnail, None);
        assert_eq!(metadata.duration, None);
        assert_eq!(metadata.author, None);
        assert_eq!(metadata.likes, None);
        assert_eq!(metadata.format_ext, "mp4");
    }

    #[test]
    fn test_extract_title() {
        let json = serde_json::json!({"title": "Test Video"});
        assert_eq!(extract_title(&json), "Test Video");
    }

    #[test]
    fn test_extract_title_default() {
        let json = serde_json::json!({});
        assert_eq!(extract_title(&json), "Unknown Title");
    }

    #[test]
    fn test_extract_id() {
        let json = serde_json::json!({"id": "abc123"});
        assert_eq!(extract_id(&json), "abc123");
    }

    #[test]
    fn test_extract_id_default() {
        let json = serde_json::json!({});
        assert_eq!(extract_id(&json), "video");
    }

    #[test]
    fn test_extract_thumbnail() {
        let json = serde_json::json!({"thumbnail": "https://example.com/thumb.jpg"});
        assert_eq!(
            extract_thumbnail(&json),
            Some("https://example.com/thumb.jpg".to_string())
        );
    }

    #[test]
    fn test_extract_thumbnail_none() {
        let json = serde_json::json!({});
        assert!(extract_thumbnail(&json).is_none());
    }

    #[test]
    fn test_extract_duration() {
        let json = serde_json::json!({"duration": 123.5});
        assert_eq!(extract_duration(&json), Some(123));
    }

    #[test]
    fn test_extract_duration_none() {
        let json = serde_json::json!({});
        assert!(extract_duration(&json).is_none());
    }

    #[test]
    fn test_extract_author() {
        let json = serde_json::json!({"uploader": "Test Creator"});
        assert_eq!(extract_author(&json), Some("Test Creator".to_string()));
    }

    #[test]
    fn test_extract_author_none() {
        let json = serde_json::json!({});
        assert!(extract_author(&json).is_none());
    }

    #[test]
    fn test_extract_likes() {
        let json = serde_json::json!({"like_count": 5000});
        assert_eq!(extract_likes(&json), Some(5000));
    }

    #[test]
    fn test_extract_likes_none() {
        let json = serde_json::json!({});
        assert!(extract_likes(&json).is_none());
    }

    #[test]
    fn test_extract_extension_mp4() {
        let json = serde_json::json!({"ext": "mp4"});
        assert_eq!(extract_extension(&json), "mp4");
    }

    #[test]
    fn test_extract_extension_default() {
        let json = serde_json::json!({});
        assert_eq!(extract_extension(&json), "mp4");
    }
}
