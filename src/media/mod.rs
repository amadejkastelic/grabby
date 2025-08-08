mod downloader;
mod gallery_dl;
mod types;
mod ytdlp;

pub use downloader::Downloader;
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

        // Create downloader instances in priority order (yt-dlp first, then gallery-dl)
        let downloaders: Vec<Box<dyn Downloader>> = vec![
            Box::new(YtDlpDownloader::new()),
            Box::new(GalleryDlDownloader::new()),
        ];

        Ok(Self { downloaders })
    }

    pub async fn download(&self, url: &str) -> Result<MediaInfo> {
        info!("Starting download for URL: {}", url);

        let mut errors = Vec::new();

        // Try each downloader in order
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

        // If all downloaders failed
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
