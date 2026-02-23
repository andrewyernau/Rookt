use std::path::PathBuf;

/// Main configuration for the PGN extraction pipeline.
pub struct Config {
    /// URLs of .pgn.zst datasets to process (in order).
    pub dataset_urls: Vec<String>,
    /// Base output directory.
    pub output_dir: PathBuf,
    /// Temporary directory for downloaded .zst files.
    pub temp_dir: PathBuf,
    /// Path to the SQLite index database.
    pub db_path: PathBuf,
    /// Event header filter (e.g., "Rated Blitz game").
    pub event_filter: String,
    /// Optional TimeControl filter (e.g., Some("300+0")). None = accept any.
    pub time_control_filter: Option<String>,
    /// Minimum number of full moves (each side) for a game to be valid.
    pub min_full_moves: u32,
    /// Minimum valid games per player per month to qualify.
    pub min_monthly_games: u32,
    /// Minimum total valid games per player across all datasets.
    pub min_total_games: u32,
    /// Maximum in-memory buffer size (bytes) before flushing to disk.
    pub write_buffer_max_bytes: usize,
}

impl Config {
    /// Default configuration for Rated Blitz 300+0, Lichess 2025.
    pub fn default_blitz_300() -> Self {
        let base = PathBuf::from(r"D:\pgn_output");
        Self {
            dataset_urls: (1..=12)
                .map(|m| {
                    format!(
                        "https://database.lichess.org/standard/lichess_db_standard_rated_2025-{:02}.pgn.zst",
                        m
                    )
                })
                .collect(),
            temp_dir: base.join("temp"),
            db_path: base.join("index.db"),
            output_dir: base,
            event_filter: "Rated Blitz game".into(),
            time_control_filter: Some("300+0".into()),
            min_full_moves: 30,
            min_monthly_games: 25,
            min_total_games: 100,
            write_buffer_max_bytes: 2 * 1024 * 1024 * 1024, // 2 GB
        }
    }

    /// Directory where per-player .pgn.zst files are stored.
    pub fn players_dir(&self) -> PathBuf {
        self.output_dir.join("players")
    }
}
