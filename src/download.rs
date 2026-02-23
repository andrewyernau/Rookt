use crate::events::{EventSink, UiEvent};
use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// Download a file from `url` to `dest` with progress reported through `sink`.
/// Skips download if `dest` already exists and is non-empty.
pub fn download(url: &str, dest: &Path, sink: &dyn EventSink) -> Result<()> {
    if dest.exists() && fs::metadata(dest).map(|m| m.len() > 0).unwrap_or(false) {
        sink.send(UiEvent::Log(format!("Already downloaded: {}", dest.display())));
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    sink.send(UiEvent::Log(format!("Downloading: {}", url)));

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(86400))) // 24h for large files
        .build()
        .new_agent();

    let resp = agent
        .get(url)
        .call()
        .context("HTTP request failed")?;

    let total_size: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    sink.send(UiEvent::DownloadStarted { total_bytes: total_size });

    let tmp_dest = dest.with_extension("zst.part");
    let mut file = fs::File::create(&tmp_dest).context("Failed to create temp file")?;
    let mut reader = resp.into_body().into_reader();
    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    let mut last_report = 0u64;

    loop {
        if downloaded % (10 * 1024 * 1024) < 65536 {
            sink.check()?;
        }

        let n = reader.read(&mut buffer).context("Network read error")?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        downloaded += n as u64;

        if downloaded - last_report > 1_048_576 {
            sink.send(UiEvent::DownloadProgress { bytes_read: downloaded });
            last_report = downloaded;
        }
    }

    file.flush()?;
    drop(file);
    fs::rename(&tmp_dest, dest).context("Failed to rename temp file")?;

    sink.send(UiEvent::DownloadComplete { size_bytes: downloaded });
    Ok(())
}
