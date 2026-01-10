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

    /// Parses gallery-dl JSON output and extracts metadata and URLs.
    fn parse_json(json_value: &Value) -> Result<(MediaMetadata, Vec<String>)> {
        let array: &Vec<Value> = json_value
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid media metadata format: expected array"))?;

        if array.is_empty() {
            return Err(anyhow::anyhow!("No media found for this URL"));
        }

        let mut urls = Vec::new();
        let mut metadata = None;

        for item in array {
            let Some(item_array) = item.as_array() else {
                continue;
            };

            if item_array.len() != 3 {
                continue;
            }

            let (Some(url_val), Some(meta)) = (item_array.get(1), item_array.get(2)) else {
                continue;
            };

            let Some(url_str) = url_val.as_str() else {
                continue;
            };

            urls.push(url_str.to_string());

            if metadata.is_some() {
                continue;
            }

            metadata = Some(MediaMetadata {
                title: extract_title(meta),
                id: extract_id(meta),
                thumbnail: None,
                duration: None,
                author: extract_author(meta),
                likes: extract_likes(meta),
                format_ext: extract_extension(meta),
            });
        }

        let metadata = metadata.ok_or_else(|| anyhow::anyhow!("No media metadata found"))?;

        if urls.is_empty() {
            return Err(anyhow::anyhow!("No media URLs found"));
        }

        debug!("Found {} media URLs", urls.len());
        Ok((metadata, urls))
    }

    async fn extract_metadata_and_urls(&self, url: &str) -> Result<(MediaMetadata, Vec<String>)> {
        debug!("Extracting metadata with gallery-dl for: {}", url);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("gallery-dl")
                .arg("--dump-json")
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
        debug!("gallery-dl raw JSON output: {}", json_str);

        let json_array: Value =
            serde_json::from_str(&json_str).context("Failed to parse media metadata")?;

        Self::parse_json(&json_array)
    }

    async fn download_url_to_memory(
        &self,
        url: &str,
        index: usize,
        metadata: &MediaMetadata,
    ) -> Result<MediaFile> {
        debug!("Downloading URL to memory: {}", url);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .context("Failed to create HTTP client")?;

        let response = client
            .get(url)
            .send()
            .await
            .context("Failed to fetch media URL")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download media: HTTP {}",
                response.status()
            ));
        }

        let data = response
            .bytes()
            .await
            .context("Failed to read media data")?
            .to_vec();

        let filename = if index == 0 {
            format!("{}.{}", metadata.id, metadata.format_ext)
        } else {
            format!("{}_{}.{}", metadata.id, index + 1, metadata.format_ext)
        };

        Ok(MediaFile { filename, data })
    }
}

fn extract_author(meta: &Value) -> Option<String> {
    if let Some(author_obj) = meta["author"].as_object() {
        author_obj
            .get("nick")
            .and_then(|v| v.as_str())
            .or_else(|| author_obj.get("name").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        meta["author"]
            .as_str()
            .or(meta["uploader"].as_str())
            .map(|s| s.to_string())
    }
}

fn extract_id(meta: &Value) -> String {
    meta["tweet_id"]
        .as_str()
        .or(meta["id"].as_str())
        .or(meta["filename"].as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn extract_title(meta: &Value) -> String {
    meta["title"]
        .as_str()
        .or(meta["content"].as_str())
        .or(meta["filename"].as_str())
        .unwrap_or("Unknown Media")
        .to_string()
}

fn extract_likes(meta: &Value) -> Option<u64> {
    meta["ups"]
        .as_u64()
        .or(meta["score"].as_u64())
        .or(meta["favorite_count"].as_u64())
}

fn extract_extension(meta: &Value) -> String {
    meta["extension"].as_str().unwrap_or("jpg").to_string()
}

#[async_trait]
impl Downloader for GalleryDlDownloader {
    fn name(&self) -> &'static str {
        "gallery-dl"
    }

    async fn download(&self, url: &str) -> Result<MediaInfo> {
        info!("Starting gallery-dl download for: {}", url);
        debug!("Extracting metadata and URLs...");
        let (metadata, media_urls) = self.extract_metadata_and_urls(url).await?;

        info!(
            "Downloading {} media files with gallery-dl: {}",
            media_urls.len(),
            metadata.id
        );

        let mut files = Vec::new();
        for (index, media_url) in media_urls.iter().enumerate() {
            match self
                .download_url_to_memory(media_url, index, &metadata)
                .await
            {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn twitter_tweet_json() -> Value {
        serde_json::json!([
            [2, {
                "author": {
                    "name": "test_user_123",
                    "nick": "Test User"
                },
                "tweet_id": "1234567890123456789",
                "content": "This is a test tweet content",
                "favorite_count": 1234,
                "extension": "jpg",
                "filename": "ABC123DEF456"
            }],
            [3, "https://example.com/media/ABC123DEF456.jpg", {
                "author": {
                    "name": "test_user_123",
                    "nick": "Test User"
                },
                "tweet_id": "1234567890123456789",
                "content": "This is a test tweet content",
                "favorite_count": 1234,
                "extension": "jpg",
                "filename": "ABC123DEF456"
            }]
        ])
    }

    fn youtube_video_json() -> Value {
        serde_json::json!([
            [2, {
                "id": "dQw4w9WgXcQ",
                "title": "Never Gonna Give You Up",
                "author": "Rick Astley",
                "ups": 15000000,
                "extension": "mp4",
                "filename": "video"
            }],
            [3, "https://example.com/video.mp4", {
                "id": "dQw4w9WgXcQ",
                "title": "Never Gonna Give You Up",
                "author": "Rick Astley",
                "ups": 15000000,
                "extension": "mp4",
                "filename": "video"
            }]
        ])
    }

    fn reddit_post_json() -> Value {
        serde_json::json!([
            [2, {
                "id": "xyz789",
                "title": "Test Post Title",
                "author": "test_redditor",
                "score": 2500,
                "extension": "png",
                "filename": "test_image"
            }],
            [3, "https://example.com/images/test_image.png", {
                "id": "xyz789",
                "title": "Test Post Title",
                "author": "test_redditor",
                "score": 2500,
                "extension": "png",
                "filename": "test_image"
            }]
        ])
    }

    #[test]
    fn test_parse_twitter_tweet_json() {
        let json = twitter_tweet_json();
        let result = GalleryDlDownloader::parse_json(&json);

        assert!(result.is_ok());
        let (metadata, urls) = result.unwrap();

        assert_eq!(metadata.id, "1234567890123456789");
        assert_eq!(metadata.title, "This is a test tweet content");
        assert_eq!(metadata.author, Some("Test User".to_string()));
        assert_eq!(metadata.likes, Some(1234));
        assert_eq!(metadata.format_ext, "jpg");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/media/ABC123DEF456.jpg");
    }

    #[test]
    fn test_parse_youtube_video_json() {
        let json = youtube_video_json();
        let result = GalleryDlDownloader::parse_json(&json);

        assert!(result.is_ok());
        let (metadata, urls) = result.unwrap();

        assert_eq!(metadata.id, "dQw4w9WgXcQ");
        assert_eq!(metadata.title, "Never Gonna Give You Up");
        assert_eq!(metadata.author, Some("Rick Astley".to_string()));
        assert_eq!(metadata.likes, Some(15000000));
        assert_eq!(metadata.format_ext, "mp4");
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_parse_reddit_post_json() {
        let json = reddit_post_json();
        let result = GalleryDlDownloader::parse_json(&json);

        assert!(result.is_ok());
        let (metadata, urls) = result.unwrap();

        assert_eq!(metadata.id, "xyz789");
        assert_eq!(metadata.title, "Test Post Title");
        assert_eq!(metadata.author, Some("test_redditor".to_string()));
        assert_eq!(metadata.likes, Some(2500));
        assert_eq!(metadata.format_ext, "png");
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_parse_empty_array_returns_error() {
        let json: Value = serde_json::json!([]);
        let result = GalleryDlDownloader::parse_json(&json);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No media found"));
    }

    #[test]
    fn test_parse_invalid_format_returns_error() {
        let json: Value = serde_json::json!("not an array");
        let result = GalleryDlDownloader::parse_json(&json);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid media metadata format"));
    }

    #[test]
    fn test_parse_array_without_urls_returns_error() {
        let json: Value = serde_json::json!([
            [2, {"title": "metadata only"}]
        ]);
        let result = GalleryDlDownloader::parse_json(&json);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No media metadata found"));
    }

    #[test]
    fn test_extract_author_twitter_nested() {
        let meta = serde_json::json!({
            "author": {"name": "username", "nick": "Display Name"}
        });
        let result = extract_author(&meta);
        assert_eq!(result, Some("Display Name".to_string()));
    }

    #[test]
    fn test_extract_author_twitter_nested_fallback() {
        let meta = serde_json::json!({
            "author": {"name": "username"}
        });
        let result = extract_author(&meta);
        assert_eq!(result, Some("username".to_string()));
    }

    #[test]
    fn test_extract_author_flat_string() {
        let meta = serde_json::json!({
            "author": "TestAuthor"
        });
        let result = extract_author(&meta);
        assert_eq!(result, Some("TestAuthor".to_string()));
    }

    #[test]
    fn test_extract_author_uploader_fallback() {
        let meta = serde_json::json!({
            "uploader": "TestUploader"
        });
        let result = extract_author(&meta);
        assert_eq!(result, Some("TestUploader".to_string()));
    }

    #[test]
    fn test_extract_author_none() {
        let meta = serde_json::json!({});
        let result = extract_author(&meta);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_id_tweet_id() {
        let meta = serde_json::json!({"tweet_id": "12345"});
        assert_eq!(extract_id(&meta), "12345");
    }

    #[test]
    fn test_extract_id_generic_id() {
        let meta = serde_json::json!({"id": "abc123"});
        assert_eq!(extract_id(&meta), "abc123");
    }

    #[test]
    fn test_extract_id_filename_fallback() {
        let meta = serde_json::json!({"filename": "video"});
        assert_eq!(extract_id(&meta), "video");
    }

    #[test]
    fn test_extract_id_unknown_fallback() {
        let meta = serde_json::json!({});
        assert_eq!(extract_id(&meta), "unknown");
    }

    #[test]
    fn test_extract_title_priority() {
        let meta = serde_json::json!({
            "title": "Video Title",
            "content": "Tweet content"
        });
        assert_eq!(extract_title(&meta), "Video Title");
    }

    #[test]
    fn test_extract_title_content_fallback() {
        let meta = serde_json::json!({
            "content": "Tweet content",
            "filename": "file"
        });
        assert_eq!(extract_title(&meta), "Tweet content");
    }

    #[test]
    fn test_extract_title_filename_fallback() {
        let meta = serde_json::json!({"filename": "file"});
        assert_eq!(extract_title(&meta), "file");
    }

    #[test]
    fn test_extract_title_default() {
        let meta = serde_json::json!({});
        assert_eq!(extract_title(&meta), "Unknown Media");
    }

    #[test]
    fn test_extract_likes_ups() {
        let meta = serde_json::json!({"ups": 100});
        assert_eq!(extract_likes(&meta), Some(100));
    }

    #[test]
    fn test_extract_likes_score() {
        let meta = serde_json::json!({"score": 500});
        assert_eq!(extract_likes(&meta), Some(500));
    }

    #[test]
    fn test_extract_likes_favorite_count() {
        let meta = serde_json::json!({"favorite_count": 1000});
        assert_eq!(extract_likes(&meta), Some(1000));
    }

    #[test]
    fn test_extract_likes_none() {
        let meta = serde_json::json!({});
        assert!(extract_likes(&meta).is_none());
    }

    #[test]
    fn test_extract_extension_jpg() {
        let meta = serde_json::json!({"extension": "jpg"});
        assert_eq!(extract_extension(&meta), "jpg");
    }

    #[test]
    fn test_extract_extension_default() {
        let meta = serde_json::json!({});
        assert_eq!(extract_extension(&meta), "jpg");
    }
}
