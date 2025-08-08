use super::types::MediaInfo;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Downloader: Send + Sync {
    /// Human-readable name of the downloader
    fn name(&self) -> &'static str;

    /// Download media from the given URL
    async fn download(&self, url: &str) -> Result<MediaInfo>;

    /// Test if this downloader is available on the system
    async fn test_availability() -> bool
    where
        Self: Sized;

    /// Optional: Check if this downloader is preferred for a specific URL
    /// Default implementation returns false (no preference)
    #[allow(dead_code)]
    fn is_preferred_for_url(&self, _url: &str) -> bool {
        false
    }
}
