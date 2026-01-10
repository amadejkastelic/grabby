use anyhow::Result;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;
use tracing::{debug, info};

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

    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(input_path)
        .arg("-vf")
        .arg("scale=iw*min(1\\,min(1280/iw\\,720/ih)):ih*min(1\\,min(1280/iw\\,720/ih))")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("medium")
        .arg("-crf")
        .arg("28")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("128k")
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
        let result = resize_image_file(&data, "test.jpg", 25);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
        assert_eq!(resized, data);
    }

    #[test]
    fn test_resize_image_file_exactly_at_limit() {
        let data = vec![0; 25_000_000];
        let result = resize_image_file(&data, "test.jpg", 25);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
    }

    #[test]
    fn test_resize_media_file_within_limit() {
        let data = create_small_test_data();
        let result = resize_media_file(&data, "test.mp4", 25);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
        assert_eq!(resized, data);
    }

    #[test]
    fn test_resize_media_file_exactly_at_limit() {
        let data = vec![0; 25_000_000];
        let result = resize_media_file(&data, "test.mp4", 25);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert_eq!(resized.len(), data.len());
    }

    #[test]
    #[ignore = "Requires ffmpeg installed"]
    fn test_resize_image_file_exceeds_limit() {
        let data = vec![0; 30_000_000];
        let result = resize_image_file(&data, "test.jpg", 25);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert!(resized.len() < data.len());
    }

    #[test]
    #[ignore = "Requires ffmpeg installed"]
    fn test_resize_media_file_exceeds_limit() {
        let data = vec![0; 30_000_000];
        let result = resize_media_file(&data, "test.mp4", 25);

        assert!(result.is_ok());
        let resized = result.unwrap();
        assert!(resized.len() < data.len());
    }
}
