use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Condvar, Mutex,
};

// ── Events from pipeline to UI ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum UiEvent {
    Log(String),

    DatasetStarted { index: usize, total: usize, name: String },
    DatasetSkipped { name: String },
    DatasetComplete,

    DownloadStarted { total_bytes: u64 },
    DownloadProgress { bytes_read: u64 },
    DownloadComplete { size_bytes: u64 },

    /// Progress reading the compressed .zst file (pass 1 or pass 2).
    FileProgress { bytes_read: u64, total_bytes: u64 },

    Pass1Started,
    Pass1Progress { games_scanned: u64, valid_games: u64, unique_players: u64 },
    Pass1Complete {
        total_scanned: u64,
        valid_games: u64,
        qualifying_players: u64,
        qualifying_games: u64,
    },

    Pass2Started,
    Pass2Progress { games_extracted: u64 },
    Pass2Complete { total_extracted: u64 },

    PruneStarted { to_remove: u64 },
    PruneComplete { remaining: u64, removed: u64 },

    Finished,
    Error(String),
}

// ── Pipeline control (pause / cancel) ───────────────────────────────────────

pub struct PipelineControl {
    paused: AtomicBool,
    cancelled: AtomicBool,
    lock: Mutex<()>,
    cvar: Condvar,
}

impl PipelineControl {
    pub fn new() -> Self {
        Self {
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            lock: Mutex::new(()),
            cvar: Condvar::new(),
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cvar.notify_all();
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.resume(); // unblock if paused
    }

    #[allow(dead_code)]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Blocks while paused. Returns `Err` if cancelled.
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            anyhow::bail!("Cancelled by user");
        }
        while self.paused.load(Ordering::SeqCst) {
            let guard = self.lock.lock().unwrap();
            let _guard = self.cvar.wait(guard).unwrap();
            if self.cancelled.load(Ordering::SeqCst) {
                anyhow::bail!("Cancelled by user");
            }
        }
        Ok(())
    }
}

// ── EventSink trait ─────────────────────────────────────────────────────────

/// Abstraction for sending pipeline events.
pub trait EventSink: Send + Sync {
    fn send(&self, event: UiEvent);
    /// Check for pause/cancel. Blocks while paused. Returns Err if cancelled.
    fn check(&self) -> Result<()>;
}

// ── Console sink (headless mode) ────────────────────────────────────────────

pub struct ConsoleSink {
    pb: Mutex<Option<ProgressBar>>,
}

impl ConsoleSink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            pb: Mutex::new(None),
        })
    }

    fn make_pb(total: u64, template: &str) -> ProgressBar {
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(template)
                .unwrap()
                .progress_chars("#>-"),
        );
        pb
    }
}

impl EventSink for ConsoleSink {
    fn send(&self, event: UiEvent) {
        match event {
            UiEvent::Log(msg) => println!("  {}", msg),

            UiEvent::DatasetStarted { index, total, name } => {
                println!("\n━━━ [{}/{}] {} ━━━", index + 1, total, name);
            }
            UiEvent::DatasetSkipped { name } => {
                println!("  Already processed: {}", name);
            }
            UiEvent::DatasetComplete => {}

            UiEvent::DownloadStarted { total_bytes } => {
                let pb = Self::make_pb(
                    total_bytes,
                    "  DL {spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA: {eta})",
                );
                *self.pb.lock().unwrap() = Some(pb);
            }
            UiEvent::DownloadProgress { bytes_read } => {
                if let Some(pb) = self.pb.lock().unwrap().as_ref() {
                    pb.set_position(bytes_read);
                }
            }
            UiEvent::DownloadComplete { size_bytes } => {
                if let Some(pb) = self.pb.lock().unwrap().take() {
                    pb.finish_and_clear();
                }
                println!(
                    "  Downloaded ({:.2} GB)",
                    size_bytes as f64 / 1_073_741_824.0
                );
            }

            UiEvent::FileProgress { bytes_read, total_bytes } => {
                let mut guard = self.pb.lock().unwrap();
                if guard.is_none() {
                    *guard = Some(Self::make_pb(
                        total_bytes,
                        "    {spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA: {eta})",
                    ));
                }
                if let Some(pb) = guard.as_ref() {
                    pb.set_position(bytes_read);
                }
            }

            UiEvent::Pass1Started => println!("  Pass 1: Counting valid games per player..."),
            UiEvent::Pass1Progress { games_scanned, unique_players, .. } => {
                if games_scanned % 1_000_000 == 0 {
                    eprint!(
                        "\r    Scanned {}M games, {} players...   ",
                        games_scanned / 1_000_000,
                        unique_players
                    );
                }
            }
            UiEvent::Pass1Complete {
                total_scanned,
                valid_games,
                qualifying_players,
                qualifying_games,
            } => {
                if let Some(pb) = self.pb.lock().unwrap().take() {
                    pb.finish_and_clear();
                }
                eprintln!();
                println!("    {} total games scanned", total_scanned);
                println!(
                    "    {} valid, {} qualifying players, {} games to extract",
                    valid_games, qualifying_players, qualifying_games
                );
            }

            UiEvent::Pass2Started => println!("  Pass 2: Extracting games..."),
            UiEvent::Pass2Progress { games_extracted } => {
                if games_extracted > 0 && games_extracted % 500_000 == 0 {
                    eprint!("\r    Extracted {} entries...   ", games_extracted);
                }
            }
            UiEvent::Pass2Complete { total_extracted } => {
                if let Some(pb) = self.pb.lock().unwrap().take() {
                    pb.finish_and_clear();
                }
                eprintln!();
                println!("    Extracted {} total game entries.", total_extracted);
            }

            UiEvent::PruneStarted { to_remove } => {
                println!("\n━━━ Final Pruning ━━━");
                println!("  Removing {} players below threshold...", to_remove);
            }
            UiEvent::PruneComplete { remaining, removed } => {
                println!(
                    "  Removed {}. {} qualifying players remain.",
                    removed, remaining
                );
            }

            UiEvent::Finished => println!("\n=== Complete ==="),
            UiEvent::Error(msg) => eprintln!("\n  ERROR: {}", msg),
        }
    }

    fn check(&self) -> Result<()> {
        Ok(())
    }
}

// ── Channel sink (TUI mode) ────────────────────────────────────────────────

pub struct ChannelSink {
    tx: mpsc::Sender<UiEvent>,
    control: Arc<PipelineControl>,
}

impl ChannelSink {
    pub fn new(tx: mpsc::Sender<UiEvent>, control: Arc<PipelineControl>) -> Arc<Self> {
        Arc::new(Self { tx, control })
    }
}

impl EventSink for ChannelSink {
    fn send(&self, event: UiEvent) {
        let _ = self.tx.send(event);
    }

    fn check(&self) -> Result<()> {
        self.control.check()
    }
}
