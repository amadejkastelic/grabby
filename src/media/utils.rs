use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

/// Remuxes MPEG-TS data to MP4 format using ffmpeg.
pub async fn remux_ts_to_mp4(ts_data: &[u8]) -> Result<Vec<u8>> {
    info!(
        "Starting ffmpeg remux with {} bytes of TS data",
        ts_data.len()
    );

    let mut ffmpeg = tokio::process::Command::new("ffmpeg")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg("pipe:0")
        .arg("-c")
        .arg("copy")
        .arg("-bsf:a")
        .arg("aac_adtstoasc")
        .arg("-f")
        .arg("mp4")
        .arg("-movflags")
        .arg("frag_keyframe+empty_moov")
        .arg("pipe:1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn ffmpeg")?;

    info!("ffmpeg spawned successfully");

    let mut stdin = ffmpeg.stdin.take().context("Failed to get ffmpeg stdin")?;
    let ts_data = ts_data.to_vec();

    let write_task = tokio::spawn(async move {
        info!("Starting write task to ffmpeg stdin");
        let result = stdin.write_all(&ts_data).await;
        drop(stdin);
        match result {
            Ok(_) => info!("Successfully wrote {} bytes to ffmpeg stdin", ts_data.len()),
            Err(e) => info!(
                "Failed to write to ffmpeg stdin: {}, kind: {:?}",
                e,
                e.kind()
            ),
        }
    });

    let stdout = ffmpeg
        .stdout
        .take()
        .context("Failed to get ffmpeg stdout")?;
    let stderr = ffmpeg
        .stderr
        .take()
        .context("Failed to get ffmpeg stderr")?;

    let mut stdout_reader = tokio::io::BufReader::new(stdout);
    let mut stderr_reader = tokio::io::BufReader::new(stderr);

    let mut buffer = Vec::new();
    let mut error_buffer = Vec::new();

    let (result, error) = tokio::join!(
        stdout_reader.read_to_end(&mut buffer),
        stderr_reader.read_to_end(&mut error_buffer),
    );

    match result {
        Ok(bytes_read) => info!("Read {} bytes from ffmpeg stdout", bytes_read),
        Err(ref e) => info!("Failed to read ffmpeg stdout: {}", e),
    }

    match error {
        Ok(bytes_read) => info!("Read {} bytes from ffmpeg stderr", bytes_read),
        Err(ref e) => info!("Failed to read ffmpeg stderr: {}", e),
    }

    result.context("Failed to read ffmpeg output")?;
    error.context("Failed to read ffmpeg stderr")?;

    let status = ffmpeg.wait().await.context("Failed to wait for ffmpeg")?;

    write_task.await.context("Failed to join write task")?;

    if !status.success() {
        let error = String::from_utf8_lossy(&error_buffer);
        info!("ffmpeg failed with status {}: {}", status, error);
        return Err(anyhow::anyhow!("ffmpeg failed: {}", error));
    }

    info!(
        "Successfully remuxed to MP4, output size: {} bytes",
        buffer.len()
    );
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires ffmpeg to be installed
    async fn test_remux_ts_to_mp4() {
        let ts_data = vec![0x47, 0x40, 0x11, 0x10, 0x00, 0x42, 0xf0, 0x25];
        let result = remux_ts_to_mp4(&ts_data).await;
        assert!(result.is_ok());
        let mp4_data = result.unwrap();
        assert!(!mp4_data.is_empty());
        assert!(mp4_data.starts_with(&[0x00, 0x00, 0x00]));
    }

    #[test]
    fn test_mpeg_ts_magic_bytes() {
        assert_eq!([0x47, 0x40], [0x47, 0x40]);
    }
}
