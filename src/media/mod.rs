mod downloader;
mod gallery_dl;
mod resize;
mod types;
mod ytdlp;

pub use downloader::Downloader;
pub use resize::{resize_image_file, resize_media_file};
pub use types::MediaInfo;

use anyhow::Result;
use gallery_dl::GalleryDlDownloader;
use tracing::{info, warn};
use ytdlp::YtDlpDownloader;

pub struct MediaDownloader {
    downloaders: Vec<Box<dyn Downloader>>,
}

impl MediaDownloader {
    pub fn new() -> Result<Self> {
        info!(
            "Media downloader initialized - using in-memory downloads with yt-dlp and gallery-dl"
        );

        // Create downloader instances in priority order (gallery-dl first, then yt-dlp)
        let downloaders: Vec<Box<dyn Downloader>> = vec![
            // gallery-dl is tried first as it also has yt-dlp integration
            Box::new(GalleryDlDownloader::new()),
            Box::new(YtDlpDownloader::new()),
        ];

        Ok(Self { downloaders })
    }

    pub async fn download(&self, url: &str) -> Result<MediaInfo> {
        info!("Starting download for URL: {}", url);

        let mut errors = Vec::new();

        for downloader in &self.downloaders {
            match downloader.download(url).await {
                Ok(media_info) => {
                    info!("Successfully downloaded with {}", downloader.name());
                    return Ok(media_info);
                }
                Err(e) => {
                    warn!("{} failed: {}", downloader.name(), e);
                    errors.push(format!("{e}"));
                }
            }
        }

        Err(anyhow::anyhow!(
            "Media download failed: {}",
            errors.join(". ")
        ))
    }

    pub fn is_supported_url(&self, _url: &str) -> bool {
        // For /embed command, assume all URLs are supported
        // The individual downloaders will handle validation and error reporting
        true
    }

    pub async fn test_setup(&self) -> Result<()> {
        info!("Testing media downloader setup...");

        let ytdlp_available = YtDlpDownloader::test_availability().await;
        let gallery_dl_available = GalleryDlDownloader::test_availability().await;

        if ytdlp_available || gallery_dl_available {
            info!("✅ At least one media downloader is available");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "No media downloaders are available. Please install yt-dlp and/or gallery-dl."
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_downloader_new() {
        let downloader = MediaDownloader::new();
        assert!(downloader.is_ok());
        let dl = downloader.unwrap();
        assert_eq!(dl.downloaders.len(), 2);
    }

    #[test]
    fn test_is_supported_url() {
        let downloader = MediaDownloader::new().unwrap();
        assert!(downloader.is_supported_url("https://example.com/video.mp4"));
        assert!(downloader.is_supported_url("https://x.com/user/status/123"));
        assert!(downloader.is_supported_url("https://youtube.com/watch?v=123"));
        assert!(downloader.is_supported_url(""));
    }
}
