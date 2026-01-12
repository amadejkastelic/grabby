use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;
use tracing::{debug, info};

fn get_video_duration(input_path: &std::path::Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(input_path)
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get video duration: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let duration: f64 = duration_str
        .trim()
        .parse()
        .context("Failed to parse video duration")?;

    Ok(duration)
}

pub fn resize_media_file(data: &[u8], filename: &str, max_size_mb: u64) -> Result<Vec<u8>> {
    let current_size = data.len() as u64;
    let max_size_bytes = max_size_mb * 1_000_000;

    if current_size <= max_size_bytes {
        debug!(
            "File {} ({} bytes) is within size limit",
            filename, current_size
        );
        return Ok(data.to_vec());
    }

    info!(
        "Resizing {} ({} bytes, {:.2} MB) to fit within {} MB limit",
        filename,
        current_size,
        current_size as f64 / 1_000_000.0,
        max_size_mb
    );

    let mut input_file = NamedTempFile::new()?;
    input_file.write_all(data)?;
    let input_path = input_file.path();

    let output_ext = if filename.ends_with(".mp4") {
        "mp4"
    } else if filename.ends_with(".webm") {
        "webm"
    } else if filename.ends_with(".mov") {
        "mov"
    } else {
        "mp4"
    };

    let output_file = NamedTempFile::with_suffix(format!(".{}", output_ext))?;
    let output_path = output_file.path();

    let duration = get_video_duration(input_path)?;
    let target_size_bytes = max_size_mb * 1_000_000;
    let target_bitrate = (target_size_bytes * 8) / duration as u64;

    let video_bitrate = target_bitrate * 9 / 10;
    let audio_bitrate = target_bitrate / 10;

    info!(
        "Video duration: {:.2}s, target bitrate: {} kbps (video: {} kbps, audio: {} kbps)",
        duration,
        target_bitrate / 1000,
        video_bitrate / 1000,
        audio_bitrate / 1000
    );

    let pass1_output = NamedTempFile::with_suffix(format!(".{}.log", output_ext))?;

    let pass1 = Command::new("ffmpeg")
        .arg("-i")
        .arg(input_path)
        .arg("-vf")
        .arg("scale='min(720\\,iw*2/2):min(480\\,ih*2/2):force_original_aspect_ratio=decrease'")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("slow")
        .arg("-b:v")
        .arg(format!("{}k", video_bitrate / 1000))
        .arg("-pass")
        .arg("1")
        .arg("-f")
        .arg("null")
        .arg("-y")
        .arg(pass1_output.path())
        .output()?;

    if !pass1.status.success() {
        anyhow::bail!(
            "Failed to encode video pass 1: {}",
            String::from_utf8_lossy(&pass1.stderr)
        );
    }

    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(input_path)
        .arg("-vf")
        .arg("scale='min(720\\,iw*2/2):min(480\\,ih*2/2):force_original_aspect_ratio=decrease'")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("slow")
        .arg("-b:v")
        .arg(format!("{}k", video_bitrate / 1000))
        .arg("-pass")
        .arg("2")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg(format!("{}k", audio_bitrate / 1000))
        .arg("-movflags")
        .arg("+faststart")
        .arg("-y")
        .arg(output_path)
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to resize video: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let resized_data = std::fs::read(output_path)?;
    let new_size = resized_data.len() as u64;

    info!(
        "Resized {} from {:.2} MB to {:.2} MB",
        filename,
        current_size as f64 / 1_000_000.0,
        new_size as f64 / 1_000_000.0
    );

    Ok(resized_data)
}

pub fn resize_image_file(data: &[u8], filename: &str, max_size_mb: u64) -> Result<Vec<u8>> {
    let current_size = data.len() as u64;
    let max_size_bytes = max_size_mb * 1_000_000;

    if current_size <= max_size_bytes {
        debug!(
            "File {} ({} bytes) is within size limit",
            filename, current_size
        );
        return Ok(data.to_vec());
    }

    info!(
        "Resizing {} ({} bytes, {:.2} MB) to fit within {} MB limit",
        filename,
        current_size,
        current_size as f64 / 1_000_000.0,
        max_size_mb
    );

    let mut input_file = NamedTempFile::new()?;
    input_file.write_all(data)?;
    let input_path = input_file.path();

    let output_ext = if filename.ends_with(".png") {
        "png"
    } else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
        "jpg"
    } else if filename.ends_with(".webp") {
        "webp"
    } else {
        "jpg"
    };

    let output_file = NamedTempFile::with_suffix(format!(".{}", output_ext))?;
    let output_path = output_file.path();

    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(input_path)
        .arg("-vf")
        .arg("scale=iw*min(1\\,min(1280/iw\\,720/ih)):ih*min(1\\,min(1280/iw\\,720/ih))")
        .arg("-quality")
        .arg("85")
        .arg("-y")
        .arg(output_path)
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to resize image: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let resized_data = std::fs::read(output_path)?;
    let new_size = resized_data.len() as u64;

    info!(
        "Resized {} from {:.2} MB to {:.2} MB",
        filename,
        current_size as f64 / 1_000_000.0,
        new_size as f64 / 1_000_000.0
    );

    Ok(resized_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_small_test_data() -> Vec<u8> {
        vec![0; 1_000_000]
    }

    #[test]
    fn test_resize_image_file_within_limit() {
        let data = create_small_test_data();
        let result = resize_image_file(&data, "test.jpg", 10);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
        assert_eq!(resized, data);
    }

    #[test]
    fn test_resize_image_file_exactly_at_limit() {
        let data = vec![0; 10_000_000];
        let result = resize_image_file(&data, "test.jpg", 10);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
    }

    #[test]
    fn test_resize_media_file_within_limit() {
        let data = create_small_test_data();
        let result = resize_media_file(&data, "test.mp4", 10);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
        assert_eq!(resized, data);
    }

    #[test]
    fn test_resize_media_file_exactly_at_limit() {
        let data = vec![0; 10_000_000];
        let result = resize_media_file(&data, "test.mp4", 10);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
    }

    #[test]
    #[ignore = "Requires ffmpeg installed"]
    fn test_resize_image_file_exceeds_limit() {
        let data = vec![0; 30_000_000];
        let result = resize_image_file(&data, "test.jpg", 10);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert!(resized.len() < data.len());
    }

    #[test]
    #[ignore = "Requires ffmpeg installed"]
    fn test_resize_media_file_exceeds_limit() {
        let data = vec![0; 30_000_000];
        let result = resize_media_file(&data, "test.mp4", 10);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert!(resized.len() < data.len());
    }
}
