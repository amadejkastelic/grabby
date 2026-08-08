mod downloader;
mod gallery_dl;
mod resize;
mod types;
mod utils;
mod ytdlp;

pub use downloader::Downloader;
pub use resize::{resize_image_file, resize_media_file};
pub use types::MediaInfo;
pub use utils::remux_ts_to_mp4;

use anyhow::Result;
use gallery_dl::GalleryDlDownloader;
use tracing::{info, warn};
use ytdlp::YtDlpDownloader;

const URL_TRANSFORMS: &[(&str, &[&str])] = &[
    (
        "instagram.com",
        &["kkinstagram.com", "ddinstagram.com", "oginstagram.com"],
    ),
    (
        "instagr.am",
        &["kkinstagram.com", "ddinstagram.com", "oginstagram.com"],
    ),
    ("tiktok.com", &["kktiktok.com", "vxtiktok.com"]),
    ("x.com", &["fxtwitter.com", "vxtwitter.com", "fixupx.com"]),
    (
        "twitter.com",
        &["fxtwitter.com", "vxtwitter.com", "fixupx.com"],
    ),
    ("reddit.com", &["vxreddit.com", "rxddit.com"]),
];

pub fn get_mirrors(url: &str) -> Option<(url::Url, &'static [&'static str])> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    for (pattern, mirrors) in URL_TRANSFORMS {
        if host == *pattern || host.ends_with(&format!(".{pattern}")) {
            return Some((parsed, mirrors));
        }
    }
    None
}

pub fn transform_to_host(parsed: &url::Url, host: &str) -> Option<String> {
    let mut new_url = parsed.clone();
    new_url.set_host(Some(host)).ok()?;
    Some(new_url.to_string())
}

pub fn get_transformed_url(url: &str) -> Option<String> {
    let (parsed, mirrors) = get_mirrors(url)?;
    let first = *mirrors.first()?;
    transform_to_host(&parsed, first)
}

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
        info!(url = %url, "Starting download");

        let mut errors = Vec::new();

        for downloader in &self.downloaders {
            match downloader.download(url).await {
                Ok(media_info) => {
                    info!(
                        url = %url,
                        downloader = downloader.name(),
                        "Successfully downloaded media"
                    );
                    return Ok(media_info);
                }
                Err(e) => {
                    warn!(
                        url = %url,
                        downloader = downloader.name(),
                        error = %e,
                        "Downloader failed"
                    );
                    errors.push(format!("{e}"));
                }
            }
        }

        Err(anyhow::anyhow!(
            "Media download failed: {}",
            errors.join(". ")
        ))
    }

    pub fn get_transformed_url(&self, url: &str) -> Option<String> {
        get_transformed_url(url)
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

    #[test]
    fn test_transform_reddit_basic() {
        assert_eq!(
            get_transformed_url("https://reddit.com/r/test/comments/abc/"),
            Some("https://vxreddit.com/r/test/comments/abc/".to_string())
        );
    }

    #[test]
    fn test_transform_reddit_old_subdomain() {
        assert_eq!(
            get_transformed_url("https://old.reddit.com/r/test/comments/abc/"),
            Some("https://vxreddit.com/r/test/comments/abc/".to_string())
        );
    }

    #[test]
    fn test_transform_reddit_www_subdomain() {
        assert_eq!(
            get_transformed_url("https://www.reddit.com/r/test/comments/abc/"),
            Some("https://vxreddit.com/r/test/comments/abc/".to_string())
        );
    }

    #[test]
    fn test_transform_tiktok_basic() {
        assert_eq!(
            get_transformed_url("https://www.tiktok.com/@user/video/123"),
            Some("https://kktiktok.com/@user/video/123".to_string())
        );
    }

    #[test]
    fn test_transform_tiktok_vm_subdomain() {
        assert_eq!(
            get_transformed_url("https://vm.tiktok.com/ZMhAbCdEf/"),
            Some("https://kktiktok.com/ZMhAbCdEf/".to_string())
        );
    }

    #[test]
    fn test_transform_x_to_fxtwitter() {
        assert_eq!(
            get_transformed_url("https://x.com/user/status/123456"),
            Some("https://fxtwitter.com/user/status/123456".to_string())
        );
    }

    #[test]
    fn test_transform_twitter_to_fxtwitter() {
        assert_eq!(
            get_transformed_url("https://twitter.com/user/status/123456"),
            Some("https://fxtwitter.com/user/status/123456".to_string())
        );
    }

    #[test]
    fn test_transform_twitter_www() {
        assert_eq!(
            get_transformed_url("https://www.twitter.com/user/status/123456"),
            Some("https://fxtwitter.com/user/status/123456".to_string())
        );
    }

    #[test]
    fn test_transform_instagram() {
        assert_eq!(
            get_transformed_url("https://www.instagram.com/p/ABC123/"),
            Some("https://kkinstagram.com/p/ABC123/".to_string())
        );
    }

    #[test]
    fn test_transform_instagram_short() {
        assert_eq!(
            get_transformed_url("https://instagr.am/p/ABC123/"),
            Some("https://kkinstagram.com/p/ABC123/".to_string())
        );
    }

    #[test]
    fn test_transform_no_match() {
        assert_eq!(get_transformed_url("https://example.com/video.mp4"), None);
    }

    #[test]
    fn test_transform_preserves_query() {
        assert_eq!(
            get_transformed_url("https://reddit.com/r/test?t=all&sort=new"),
            Some("https://vxreddit.com/r/test?t=all&sort=new".to_string())
        );
    }

    #[test]
    fn test_transform_case_insensitive() {
        assert_eq!(
            get_transformed_url("https://OLD.REDDIT.COM/r/test/"),
            Some("https://vxreddit.com/r/test/".to_string())
        );
        assert_eq!(
            get_transformed_url("https://VM.TikTok.com/ZMhAbCdEf/"),
            Some("https://kktiktok.com/ZMhAbCdEf/".to_string())
        );
    }

    #[test]
    fn test_transform_invalid_url() {
        assert_eq!(get_transformed_url("not-a-url"), None);
        assert_eq!(get_transformed_url(""), None);
    }

    #[test]
    fn test_get_mirrors_x_returns_all_candidates() {
        let (_, mirrors) = get_mirrors("https://x.com/user/status/123").unwrap();
        assert_eq!(mirrors, &["fxtwitter.com", "vxtwitter.com", "fixupx.com"]);
    }

    #[test]
    fn test_get_mirrors_reddit_two_candidates() {
        let (_, mirrors) = get_mirrors("https://www.reddit.com/r/test/").unwrap();
        assert_eq!(mirrors, &["vxreddit.com", "rxddit.com"]);
    }

    #[test]
    fn test_get_mirrors_tiktok_subdomain() {
        let (_, mirrors) = get_mirrors("https://vm.tiktok.com/ZMhAbCdEf/").unwrap();
        assert_eq!(mirrors, &["kktiktok.com", "vxtiktok.com"]);
    }

    #[test]
    fn test_get_mirrors_no_match() {
        assert!(get_mirrors("https://example.com/video.mp4").is_none());
    }

    #[test]
    fn test_get_mirrors_invalid_url() {
        assert!(get_mirrors("not-a-url").is_none());
        assert!(get_mirrors("").is_none());
    }

    #[test]
    fn test_transform_to_host_basic() {
        let parsed = url::Url::parse("https://x.com/user/status/123").unwrap();
        assert_eq!(
            transform_to_host(&parsed, "vxtwitter.com"),
            Some("https://vxtwitter.com/user/status/123".to_string())
        );
    }

    #[test]
    fn test_transform_to_host_preserves_query() {
        let parsed = url::Url::parse("https://reddit.com/r/test?t=all&sort=new").unwrap();
        assert_eq!(
            transform_to_host(&parsed, "rxddit.com"),
            Some("https://rxddit.com/r/test?t=all&sort=new".to_string())
        );
    }
}
