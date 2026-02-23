use crate::config::Config;
use crate::database::Database;
use crate::download;
use crate::events::{ConsoleSink, EventSink, UiEvent};
use crate::parser::{GameInfo, PgnParser};
use crate::writer::PlayerWriter;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;

/// Run the pipeline in headless mode (console output).
pub fn run(config: &Config) -> Result<()> {
    let sink = ConsoleSink::new();
    sink.send(UiEvent::Log(format!("Output: {}", config.output_dir.display())));
    sink.send(UiEvent::Log(format!("Event: {}", config.event_filter)));
    if let Some(tc) = &config.time_control_filter {
        sink.send(UiEvent::Log(format!("TimeControl: {}", tc)));
    }
    sink.send(UiEvent::Log(format!(
        "Moves: {} full, Monthly: {}, Total: {}",
        config.min_full_moves, config.min_monthly_games, config.min_total_games
    )));
    run_with_sink(config, sink)
}

/// Run the pipeline with a given EventSink (used by both headless and TUI).
pub fn run_with_sink(config: &Config, sink: Arc<dyn EventSink>) -> Result<()> {
    fs::create_dir_all(&config.temp_dir)?;
    fs::create_dir_all(config.players_dir())?;

    let mut db = Database::open(&config.db_path)?;
    let total = config.dataset_urls.len();

    for (i, url) in config.dataset_urls.iter().enumerate() {
        sink.check()?;
        let name = url.rsplit('/').next().unwrap_or(url).to_string();
        sink.send(UiEvent::DatasetStarted { index: i, total, name: name.clone() });

        if db.is_dataset_processed(url)? {
            sink.send(UiEvent::DatasetSkipped { name });
            continue;
        }

        let month = extract_month(url);
        let zst_path = config.temp_dir.join(format!("{}.pgn.zst", month));

        // Download
        download::download(url, &zst_path, &*sink)?;
        sink.check()?;

        // Pass 1
        sink.send(UiEvent::Pass1Started);
        let player_counts = pass1_count(&zst_path, config, sink.clone())?;

        let total_valid: u64 = player_counts.values().map(|v| *v as u64).sum();
        let qualifying: HashSet<String> = player_counts
            .iter()
            .filter(|(_, count)| **count >= config.min_monthly_games)
            .map(|(name, _)| name.clone())
            .collect();
        let qualifying_games: u64 = qualifying
            .iter()
            .filter_map(|n| player_counts.get(n))
            .map(|v| *v as u64)
            .sum();

        sink.send(UiEvent::Pass1Complete {
            total_scanned: player_counts.values().map(|v| *v as u64).sum::<u64>() + total_valid, // approximate
            valid_games: total_valid,
            qualifying_players: qualifying.len() as u64,
            qualifying_games,
        });
        sink.check()?;

        if !qualifying.is_empty() {
            // Pass 2
            sink.send(UiEvent::Pass2Started);
            let mut writer = PlayerWriter::new(config.players_dir(), config.write_buffer_max_bytes);
            let extracted = pass2_extract(&zst_path, config, &qualifying, &mut writer, sink.clone())?;
            writer.flush_all()?;
            sink.send(UiEvent::Pass2Complete { total_extracted: extracted });

            let qualifying_counts: HashMap<String, u32> = player_counts
                .into_iter()
                .filter(|(name, _)| qualifying.contains(name))
                .collect();
            db.update_player_counts(&month, &qualifying_counts)?;
        }

        db.mark_dataset_processed(url)?;

        if zst_path.exists() {
            fs::remove_file(&zst_path)?;
        }

        sink.send(UiEvent::DatasetComplete);
    }

    // Final prune
    let to_remove = db.get_players_below_total(config.min_total_games)?;
    sink.send(UiEvent::PruneStarted { to_remove: to_remove.len() as u64 });

    let writer = PlayerWriter::new(config.players_dir(), 0);
    for name in &to_remove {
        writer.delete_player(name)?;
    }
    let removed = db.remove_players_below_total(config.min_total_games)?;
    cleanup_empty_dirs(&config.players_dir())?;

    let remaining = db.get_total_qualifying_players(config.min_total_games)?;
    sink.send(UiEvent::PruneComplete {
        remaining: remaining as u64,
        removed: removed as u64,
    });

    sink.send(UiEvent::Finished);
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn extract_month(url: &str) -> String {
    let filename = url.rsplit('/').next().unwrap_or(url);
    let without_ext = filename.trim_end_matches(".pgn.zst");
    without_ext.rsplit('_').next().unwrap_or("unknown").to_string()
}

/// ProgressReader sends FileProgress events through the sink.
struct ProgressReader<R> {
    inner: R,
    read_bytes: u64,
    total_bytes: u64,
    sink: Arc<dyn EventSink>,
    last_report: u64,
}

impl<R: Read> ProgressReader<R> {
    fn new(inner: R, total_bytes: u64, sink: Arc<dyn EventSink>) -> Self {
        Self { inner, read_bytes: 0, total_bytes, sink, last_report: 0 }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read_bytes += n as u64;
        // Report every ~10 MB
        if self.read_bytes - self.last_report > 10_485_760 {
            self.sink.send(UiEvent::FileProgress {
                bytes_read: self.read_bytes,
                total_bytes: self.total_bytes,
            });
            self.last_report = self.read_bytes;
        }
        Ok(n)
    }
}

fn open_zst_reader(
    path: &Path,
    sink: Arc<dyn EventSink>,
) -> Result<BufReader<zstd::Decoder<'static, BufReader<ProgressReader<File>>>>> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let file_size = file.metadata()?.len();
    let progress = ProgressReader::new(file, file_size, sink);
    let decoder = zstd::Decoder::new(progress)?;
    Ok(BufReader::with_capacity(256 * 1024, decoder))
}

fn is_valid_game(info: &GameInfo, config: &Config) -> bool {
    if info.event != config.event_filter {
        return false;
    }
    if let Some(tc) = &config.time_control_filter {
        if info.time_control != *tc {
            return false;
        }
    }
    info.half_move_count >= config.min_full_moves * 2
}

fn pass1_count(
    zst_path: &Path,
    config: &Config,
    sink: Arc<dyn EventSink>,
) -> Result<HashMap<String, u32>> {
    let reader = open_zst_reader(zst_path, sink.clone())?;
    let mut parser = PgnParser::new(reader);
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut scanned = 0u64;
    let mut valid = 0u64;

    while let Some(info) = parser.next_info()? {
        scanned += 1;
        if scanned % 100_000 == 0 {
            sink.send(UiEvent::Pass1Progress {
                games_scanned: scanned,
                valid_games: valid,
                unique_players: counts.len() as u64,
            });
        }
        if scanned % 500_000 == 0 {
            sink.check()?;
        }

        if !is_valid_game(&info, config) {
            continue;
        }
        valid += 1;

        if !info.white.is_empty() {
            *counts.entry(info.white.clone()).or_insert(0) += 1;
        }
        if !info.black.is_empty() {
            *counts.entry(info.black).or_insert(0) += 1;
        }
    }

    sink.send(UiEvent::Pass1Progress {
        games_scanned: scanned,
        valid_games: valid,
        unique_players: counts.len() as u64,
    });
    Ok(counts)
}

fn pass2_extract(
    zst_path: &Path,
    config: &Config,
    qualifying: &HashSet<String>,
    writer: &mut PlayerWriter,
    sink: Arc<dyn EventSink>,
) -> Result<u64> {
    let reader = open_zst_reader(zst_path, sink.clone())?;
    let mut parser = PgnParser::new(reader);
    let mut extracted = 0u64;

    while let Some(game) = parser.next_game()? {
        if !is_valid_game(&game.info, config) {
            continue;
        }

        let white_ok = qualifying.contains(&game.info.white);
        let black_ok = qualifying.contains(&game.info.black);

        if white_ok {
            writer.add_game(&game.info.white, &game.raw_pgn)?;
            extracted += 1;
        }
        if black_ok {
            writer.add_game(&game.info.black, &game.raw_pgn)?;
            extracted += 1;
        }

        if extracted % 100_000 == 0 && extracted > 0 {
            sink.send(UiEvent::Pass2Progress { games_extracted: extracted });
        }
        if extracted % 500_000 == 0 {
            sink.check()?;
        }
    }

    Ok(extracted)
}

fn cleanup_empty_dirs(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            cleanup_empty_dirs(&path)?;
            if fs::read_dir(&path)?.next().is_none() {
                fs::remove_dir(&path).ok();
            }
        }
    }
    Ok(())
}
