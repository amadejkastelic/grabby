use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use tracing::{debug, info, warn};

pub struct MediaDownloader {
    download_dir: PathBuf,
}

impl MediaDownloader {
    pub fn new() -> Result<Self> {
        let download_dir = PathBuf::from("./downloads");

        // Create downloads directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&download_dir) {
            warn!("Failed to create download directory: {}", e);
        }

        info!("Media downloader initialized - using system yt-dlp");

        Ok(Self { download_dir })
    }

    pub async fn download(&self, url: &str) -> Result<MediaInfo> {
        info!("Starting download for URL: {}", url);

        // First, extract metadata without downloading
        let metadata = self.extract_metadata(url).await?;

        // Then download the actual media file
        let file_path = self.download_media(url, &metadata).await?;

        Ok(MediaInfo {
            url: url.to_string(),
            file_path: Some(file_path),
            metadata,
        })
    }

    async fn extract_metadata(&self, url: &str) -> Result<MediaMetadata> {
        debug!("Extracting metadata for: {}", url);

        let output = tokio::process::Command::new("yt-dlp")
            .arg("--dump-json")
            .arg("--no-download")
            .arg("--no-warnings")
            .arg(url)
            .output()
            .await
            .context("Failed to execute yt-dlp - make sure it's installed")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "yt-dlp metadata extraction failed: {}",
                error
            ));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let json: Value =
            serde_json::from_str(&json_str).context("Failed to parse yt-dlp JSON output")?;

        debug!("yt-dlp JSON output: {}", json_str);

        Ok(MediaMetadata {
            title: json["title"]
                .as_str()
                .unwrap_or("Unknown Title")
                .to_string(),
            thumbnail: json["thumbnail"].as_str().map(|s| s.to_string()),
            duration: json["duration"].as_f64().map(|d| d as u64),
            filesize: json["filesize"].as_u64(),
            author: json["uploader"].as_str().map(|s| s.to_string()),
            likes: json["like_count"].as_u64(),
        })
    }

    async fn download_media(&self, url: &str, metadata: &MediaMetadata) -> Result<String> {
        info!("Downloading media: {}", metadata.title);

        // Generate safe filename
        let safe_title = metadata
            .title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();

        let filename = format!("{safe_title}.%(ext)s");
        let output_template = self.download_dir.join(&filename);

        let output = tokio::process::Command::new("yt-dlp")
            .arg("-o")
            .arg(&output_template)
            .arg("--format")
            .arg("best[height<=720]/bestvideo[height<=720]+bestaudio/best[filesize<25M]/bestvideo+bestaudio/best") 
            .arg("--no-warnings")
            .arg(url)
            .output()
            .await
            .context("Failed to execute yt-dlp for media download")?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);

            // If format issue, first list available formats for debugging
            if error.contains("Requested format is not available") {
                warn!("Format not available, checking what formats are available...");

                // First check what formats are available
                let list_output = tokio::process::Command::new("yt-dlp")
                    .arg("--list-formats")
                    .arg("--no-warnings")
                    .arg(url)
                    .output()
                    .await;

                if let Ok(list_result) = list_output {
                    let formats = String::from_utf8_lossy(&list_result.stdout);
                    debug!("Available formats:\n{}", formats);
                }

                warn!("Retrying without format specification...");

                // Try without any format specification at all
                let retry_output = tokio::process::Command::new("yt-dlp")
                    .arg("-o")
                    .arg(&output_template)
                    .arg("--no-warnings")
                    .arg(url)
                    .output()
                    .await
                    .context("Failed to execute yt-dlp retry")?;

                if !retry_output.status.success() {
                    let retry_error = String::from_utf8_lossy(&retry_output.stderr);
                    return Err(anyhow::anyhow!(
                        "yt-dlp download failed even without format specification: {}",
                        retry_error
                    ));
                }
            } else {
                return Err(anyhow::anyhow!("yt-dlp download failed: {}", error));
            }
        }

        // Find the downloaded file (yt-dlp adds the extension)
        let downloaded_file = self.find_downloaded_file(&safe_title)?;

        info!("Successfully downloaded: {}", downloaded_file);
        Ok(downloaded_file)
    }

    fn find_downloaded_file(&self, base_name: &str) -> Result<String> {
        let entries =
            std::fs::read_dir(&self.download_dir).context("Failed to read download directory")?;

        for entry in entries {
            let entry = entry.context("Failed to read directory entry")?;
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if filename_str.starts_with(base_name) {
                return Ok(entry.path().to_string_lossy().to_string());
            }
        }

        Err(anyhow::anyhow!(
            "Downloaded file not found for: {}",
            base_name
        ))
    }

    pub fn is_supported_url(&self, _url: &str) -> bool {
        // For /embed command, assume all URLs are supported
        // yt-dlp will handle validation and error reporting
        true
    }

    pub async fn test_setup(&self) -> Result<()> {
        info!("Testing yt-dlp setup...");

        // Test if yt-dlp is available
        match tokio::process::Command::new("yt-dlp")
            .arg("--version")
            .output()
            .await
        {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout);
                    info!("✅ yt-dlp is available, version: {}", version.trim());
                    Ok(())
                } else {
                    warn!("❌ yt-dlp command failed");
                    Err(anyhow::anyhow!("yt-dlp is not working properly"))
                }
            }
            Err(e) => {
                warn!("❌ yt-dlp not found - please install it: {}", e);
                Err(anyhow::anyhow!("yt-dlp not installed: {}", e))
            }
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MediaMetadata {
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<u64>,
    pub filesize: Option<u64>,
    pub author: Option<String>,
    pub likes: Option<u64>,
}

#[derive(Debug)]
pub struct MediaInfo {
    pub url: String,
    pub file_path: Option<String>,
    pub metadata: MediaMetadata,
}
