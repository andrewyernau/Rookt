use anyhow::Result;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Buffered writer that accumulates games per player in memory
/// and flushes them as compressed zstd frames to per-player files.
pub struct PlayerWriter {
    players_dir: PathBuf,
    buffer: HashMap<String, Vec<u8>>,
    buffer_size: usize,
    max_buffer_size: usize,
}

impl PlayerWriter {
    pub fn new(players_dir: PathBuf, max_buffer_size: usize) -> Self {
        Self {
            players_dir,
            buffer: HashMap::new(),
            buffer_size: 0,
            max_buffer_size,
        }
    }

    /// Get the filesystem path for a player's .pgn.zst file.
    /// Sharded into subdirectories by the first 2 chars of the lowercase name.
    fn player_path(&self, name: &str) -> PathBuf {
        let lower = name.to_ascii_lowercase();
        let prefix = if lower.len() >= 2 {
            &lower[..2]
        } else {
            &lower
        };
        self.players_dir
            .join(prefix)
            .join(format!("{}.pgn.zst", name))
    }

    /// Add a game's raw PGN text to the buffer for a given player.
    /// Automatically flushes if the buffer exceeds `max_buffer_size`.
    pub fn add_game(&mut self, player: &str, pgn: &str) -> Result<()> {
        let entry = self.buffer.entry(player.to_string()).or_default();
        entry.extend_from_slice(pgn.as_bytes());
        entry.push(b'\n');
        self.buffer_size += pgn.len() + 1;

        if self.buffer_size >= self.max_buffer_size {
            self.flush_all()?;
        }
        Ok(())
    }

    /// Flush all buffered data to disk as compressed zstd frames.
    pub fn flush_all(&mut self) -> Result<()> {
        let entries: Vec<(String, Vec<u8>)> = self.buffer.drain().collect();
        for (player, data) in entries {
            if data.is_empty() {
                continue;
            }
            self.write_compressed(&player, &data)?;
        }
        self.buffer_size = 0;
        Ok(())
    }

    /// Compress `data` with zstd and append as a new frame to the player's file.
    fn write_compressed(&self, player: &str, data: &[u8]) -> Result<()> {
        let path = self.player_path(player);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let mut encoder = zstd::stream::write::Encoder::new(file, 3)?;
        encoder.write_all(data)?;
        encoder.finish()?;
        Ok(())
    }

    /// Delete a player's .pgn.zst file.
    pub fn delete_player(&self, name: &str) -> Result<()> {
        let path = self.player_path(name);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
