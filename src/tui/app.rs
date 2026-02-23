use crate::config::Config;
use crate::events::{PipelineControl, UiEvent};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

// ── Screens ─────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone)]
pub enum Screen {
    Config,
    Dashboard,
}

#[derive(PartialEq, Clone)]
pub enum RunState {
    Idle,
    Running,
    Paused,
    Finished,
    #[allow(dead_code)]
    Cancelled,
    Error(String),
}

#[derive(PartialEq, Clone)]
pub enum Phase {
    Downloading,
    Pass1,
    Pass2,
    Pruning,
    Done,
}

// ── Config field ────────────────────────────────────────────────────────────

pub struct ConfigField {
    pub label: &'static str,
    pub value: String,
    pub hint: &'static str,
}

// ── App state ───────────────────────────────────────────────────────────────

pub struct App {
    pub screen: Screen,

    // Config screen
    pub fields: Vec<ConfigField>,
    pub selected: usize,
    pub editing: bool,
    pub edit_cursor: usize,
    pub validation_error: Option<String>,

    // Dashboard state
    pub run_state: RunState,
    pub phase: Phase,
    pub current_dataset: usize,
    pub total_datasets: usize,
    pub dataset_name: String,

    // Download
    pub dl_total: u64,
    pub dl_read: u64,

    // File progress (compressed .zst bytes)
    pub file_total: u64,
    pub file_read: u64,

    // Pass 1 (current dataset)
    pub p1_scanned: u64,
    pub p1_valid: u64,
    pub p1_players: u64,

    // Pass 2 (current dataset)
    pub p2_extracted: u64,

    // Cumulative totals
    pub cum_qualifying: u64,
    pub cum_games_saved: u64,
    pub final_players: u64,

    // Logs
    pub logs: Vec<String>,
    pub log_scroll: usize,

    // Communication
    pub event_rx: Option<mpsc::Receiver<UiEvent>>,
    pub control: Option<Arc<PipelineControl>>,

    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Config,
            fields: vec![
                ConfigField { label: "Event Filter", value: "Rated Blitz game".into(), hint: "e.g. Rated Blitz game" },
                ConfigField { label: "Time Control", value: "300+0".into(), hint: "empty = any, e.g. 300+0" },
                ConfigField { label: "Min Full Moves", value: "30".into(), hint: "30 = 60 half-moves" },
                ConfigField { label: "Min Games/Month", value: "25".into(), hint: "per player per month" },
                ConfigField { label: "Min Games Total", value: "100".into(), hint: "across all datasets" },
                ConfigField { label: "Dataset Start", value: "2025-01".into(), hint: "YYYY-MM" },
                ConfigField { label: "Dataset End", value: "2025-12".into(), hint: "YYYY-MM" },
                ConfigField { label: "Output Directory", value: r"D:\pgn_output".into(), hint: "must have enough space" },
                ConfigField { label: "Buffer Size (GB)", value: "2.0".into(), hint: "RAM buffer before flush" },
            ],
            selected: 0,
            editing: false,
            edit_cursor: 0,
            validation_error: None,

            run_state: RunState::Idle,
            phase: Phase::Downloading,
            current_dataset: 0,
            total_datasets: 0,
            dataset_name: String::new(),

            dl_total: 0,
            dl_read: 0,
            file_total: 0,
            file_read: 0,
            p1_scanned: 0,
            p1_valid: 0,
            p1_players: 0,
            p2_extracted: 0,
            cum_qualifying: 0,
            cum_games_saved: 0,
            final_players: 0,

            logs: Vec::new(),
            log_scroll: 0,

            event_rx: None,
            control: None,
            should_quit: false,
        }
    }

    /// Total config fields + 1 for the Start button.
    pub fn total_items(&self) -> usize {
        self.fields.len() + 1
    }

    pub fn is_on_start_button(&self) -> bool {
        self.selected == self.fields.len()
    }

    pub fn add_log(&mut self, msg: String) {
        self.logs.push(msg);
        // Auto-scroll to bottom
        let visible = 10usize; // approximate visible log lines
        if self.logs.len() > visible {
            self.log_scroll = self.logs.len() - visible;
        }
    }

    fn reset_dataset_stats(&mut self) {
        self.dl_total = 0;
        self.dl_read = 0;
        self.file_total = 0;
        self.file_read = 0;
        self.p1_scanned = 0;
        self.p1_valid = 0;
        self.p1_players = 0;
        self.p2_extracted = 0;
    }

    /// Process a pipeline event.
    pub fn handle_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Log(msg) => self.add_log(msg),

            UiEvent::DatasetStarted { index, total, name } => {
                self.current_dataset = index;
                self.total_datasets = total;
                self.dataset_name = name.clone();
                self.reset_dataset_stats();
                self.add_log(format!("[{}/{}] Started: {}", index + 1, total, name));
            }
            UiEvent::DatasetSkipped { name } => {
                self.add_log(format!("Skipped (already done): {}", name));
            }
            UiEvent::DatasetComplete => {
                self.add_log("Dataset complete.".into());
            }

            UiEvent::DownloadStarted { total_bytes } => {
                self.phase = Phase::Downloading;
                self.dl_total = total_bytes;
                self.dl_read = 0;
            }
            UiEvent::DownloadProgress { bytes_read } => {
                self.dl_read = bytes_read;
            }
            UiEvent::DownloadComplete { size_bytes } => {
                self.dl_read = size_bytes;
                self.add_log(format!(
                    "Download complete ({:.2} GB)",
                    size_bytes as f64 / 1_073_741_824.0
                ));
            }

            UiEvent::FileProgress { bytes_read, total_bytes } => {
                self.file_read = bytes_read;
                self.file_total = total_bytes;
            }

            UiEvent::Pass1Started => {
                self.phase = Phase::Pass1;
                self.file_read = 0;
                self.file_total = 0;
                self.add_log("Pass 1: Counting games...".into());
            }
            UiEvent::Pass1Progress { games_scanned, valid_games, unique_players } => {
                self.p1_scanned = games_scanned;
                self.p1_valid = valid_games;
                self.p1_players = unique_players;
            }
            UiEvent::Pass1Complete { total_scanned, valid_games, qualifying_players, qualifying_games } => {
                self.p1_scanned = total_scanned;
                self.p1_valid = valid_games;
                self.p1_players = qualifying_players;
                self.cum_qualifying += qualifying_players;
                self.add_log(format!(
                    "Pass 1 done: {} scanned, {} valid, {} qualifying ({} games)",
                    fmt_count(total_scanned), fmt_count(valid_games),
                    fmt_count(qualifying_players), fmt_count(qualifying_games),
                ));
            }

            UiEvent::Pass2Started => {
                self.phase = Phase::Pass2;
                self.file_read = 0;
                self.file_total = 0;
                self.add_log("Pass 2: Extracting games...".into());
            }
            UiEvent::Pass2Progress { games_extracted } => {
                self.p2_extracted = games_extracted;
            }
            UiEvent::Pass2Complete { total_extracted } => {
                self.p2_extracted = total_extracted;
                self.cum_games_saved += total_extracted;
                self.add_log(format!("Pass 2 done: {} entries extracted", fmt_count(total_extracted)));
            }

            UiEvent::PruneStarted { to_remove } => {
                self.phase = Phase::Pruning;
                self.add_log(format!("Pruning {} players below threshold...", fmt_count(to_remove)));
            }
            UiEvent::PruneComplete { remaining, removed } => {
                self.final_players = remaining;
                self.add_log(format!(
                    "Prune done: {} removed, {} remaining",
                    fmt_count(removed), fmt_count(remaining),
                ));
            }

            UiEvent::Finished => {
                self.phase = Phase::Done;
                self.run_state = RunState::Finished;
                self.add_log("=== Pipeline finished ===".into());
            }

            UiEvent::Error(msg) => {
                self.run_state = RunState::Error(msg.clone());
                self.add_log(format!("ERROR: {}", msg));
            }
        }
    }

    /// Validate config fields and build a Config struct.
    pub fn build_config(&self) -> Result<Config, String> {
        let event_filter = self.fields[0].value.clone();
        if event_filter.is_empty() {
            return Err("Event filter cannot be empty".into());
        }

        let time_control = if self.fields[1].value.trim().is_empty() {
            None
        } else {
            Some(self.fields[1].value.trim().to_string())
        };

        let min_full_moves: u32 = self.fields[2].value.trim().parse()
            .map_err(|_| "Min full moves must be a positive integer")?;
        let min_monthly_games: u32 = self.fields[3].value.trim().parse()
            .map_err(|_| "Min games/month must be a positive integer")?;
        let min_total_games: u32 = self.fields[4].value.trim().parse()
            .map_err(|_| "Min games total must be a positive integer")?;

        let start = parse_month(&self.fields[5].value)?;
        let end = parse_month(&self.fields[6].value)?;
        if start > end {
            return Err("Dataset start must be before or equal to end".into());
        }

        let output_dir = PathBuf::from(self.fields[7].value.trim());
        let buffer_gb: f64 = self.fields[8].value.trim().parse()
            .map_err(|_| "Buffer size must be a number")?;
        if buffer_gb <= 0.0 {
            return Err("Buffer size must be positive".into());
        }

        let urls = generate_urls(start, end);

        Ok(Config {
            dataset_urls: urls,
            temp_dir: output_dir.join("temp"),
            db_path: output_dir.join("index.db"),
            output_dir: output_dir.clone(),
            event_filter,
            time_control_filter: time_control,
            min_full_moves,
            min_monthly_games,
            min_total_games,
            write_buffer_max_bytes: (buffer_gb * 1_073_741_824.0) as usize,
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn parse_month(s: &str) -> Result<(u32, u32), String> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid date format '{}', expected YYYY-MM", s));
    }
    let year: u32 = parts[0].parse().map_err(|_| "Invalid year")?;
    let month: u32 = parts[1].parse().map_err(|_| "Invalid month")?;
    if !(1..=12).contains(&month) {
        return Err("Month must be 1-12".into());
    }
    Ok((year, month))
}

fn generate_urls(start: (u32, u32), end: (u32, u32)) -> Vec<String> {
    let mut urls = Vec::new();
    let (mut y, mut m) = start;
    loop {
        urls.push(format!(
            "https://database.lichess.org/standard/lichess_db_standard_rated_{}-{:02}.pgn.zst",
            y, m
        ));
        if (y, m) == end {
            break;
        }
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    urls
}

pub fn fmt_count(n: u64) -> String {
    if n >= 1_000_000_000 { format!("{:.1}B", n as f64 / 1e9) }
    else if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1e6) }
    else if n >= 1_000 { format!("{:.1}K", n as f64 / 1e3) }
    else { n.to_string() }
}

pub fn fmt_bytes(n: u64) -> String {
    if n >= 1_073_741_824 { format!("{:.1} GB", n as f64 / 1_073_741_824.0) }
    else if n >= 1_048_576 { format!("{:.1} MB", n as f64 / 1_048_576.0) }
    else if n >= 1024 { format!("{:.1} KB", n as f64 / 1024.0) }
    else { format!("{} B", n) }
}
