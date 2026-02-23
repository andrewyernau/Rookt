<div align="center">

<img src="icon.png" width="140" alt="rookt logo" />

<h1>rookt</h1>

<p><strong>Lichess PGN database extractor — build per-player game archives, fast.</strong><br/>
Downloads, filters, and organises millions of chess games from the Lichess open database into individual per-player <code>.pgn.zst</code> files, with a live terminal UI.</p>

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust%202024-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Lichess](https://img.shields.io/badge/data%20source-Lichess%20DB-green?logo=lichess&logoColor=white)](https://database.lichess.org/)
[![Ko-fi](https://img.shields.io/badge/support-Ko--fi-ff5e5b?logo=ko-fi&logoColor=white)](https://ko-fi.com/andrewyernau)
[![Discord](https://img.shields.io/badge/chat-Discord-5865F2?logo=discord&logoColor=white)](https://discord.gg/939UecsD95)

</div>

---

<div align="center">
  <img src="record.gif" alt="rookt demo" width="780" />
</div>

---

## ⚡ Quickstart

```bash
# 1. Clone and build
git clone https://github.com/andrewyernau/rookt.git
cd rookt
cargo build --release

# 2. Launch the interactive TUI
./target/release/rookt

# 3. (Optional) Run headless with default settings
./target/release/rookt --headless
```

That's it. The interface will guide you through the rest.

---

## 🚀 Getting Started

### Installation

**Prerequisites:** Rust toolchain ≥ 1.85 (2024 edition). Install from [rustup.rs](https://rustup.rs/).

```bash
git clone https://github.com/andrewyernau/rookt.git
cd rookt
cargo build --release
```

The compiled binary will be at `target/release/rookt` (or `rookt.exe` on Windows).

---

### TUI Mode (recommended)

Run without arguments to launch the interactive configuration screen:

```bash
./target/release/rookt
```

You will be presented with a form to configure:

| Field | Description | Default |
|---|---|---|
| **Output dir** | Where player files and the SQLite index are saved | `D:\pgn_output` |
| **Event filter** | PGN `Event` tag to match (e.g. `Rated Blitz game`) | `Rated Blitz game` |
| **Time control** | Optional `TimeControl` filter (e.g. `300+0`). Leave empty to accept all | `300+0` |
| **Min full moves** | Minimum number of full moves for a game to be valid | `30` |
| **Min monthly games** | Minimum games a player must have per monthly dataset to qualify | `25` |
| **Min total games** | Minimum games a player must have across all datasets to keep their file | `100` |
| **Year** | Which year's Lichess monthly dumps to download | `2025` |

**Keyboard shortcuts (Config screen):**

| Key | Action |
|---|---|
| `↑` / `↓` / `Tab` | Navigate fields |
| `Enter` | Edit selected field |
| `Esc` | Confirm edit |
| `F5` or `Enter` on Start | Launch pipeline |
| `q` | Quit |

**Keyboard shortcuts (Dashboard):**

| Key | Action |
|---|---|
| `p` | Pause pipeline |
| `r` | Resume pipeline |
| `↑` / `↓` / `PgUp` / `PgDn` | Scroll log |
| `Ctrl+C` / `q` | Cancel and quit |

---

### Headless Mode

Use `--headless` to run non-interactively with the built-in default configuration (Rated Blitz 300+0, full year 2025):

```bash
./target/release/rookt --headless
```

Useful for running rookt inside scripts, Docker containers, or SSH sessions.

---

### Output Structure

```
<output_dir>/
├── index.db              ← SQLite index (tracks processed datasets & player counts)
├── temp/                 ← Temporary .zst downloads (auto-cleaned after each month)
└── players/
    ├── aa/
    │   └── AaronNimzo.pgn.zst
    ├── ab/
    │   └── Abcdefg123.pgn.zst
    └── ...               ← Sharded by first 2 characters of username (lowercase)
```

Each `<Username>.pgn.zst` file contains all of that player's qualifying games in standard PGN format, compressed with zstd. Multiple zstd frames may be appended across monthly processing runs.

---

## 🏗️ Architecture

```
rookt/
├── main.rs           — Entry point; routes to TUI or headless mode
├── config.rs         — Config struct with all pipeline parameters
├── pipeline.rs       — Core orchestrator: download → pass 1 → pass 2 → prune
├── download.rs       — HTTP downloader with progress events (ureq)
├── parser.rs         — Streaming PGN parser (zero-copy, BufRead)
├── writer.rs         — Buffered, sharded, zstd-compressed per-player writer
├── database.rs       — SQLite index (rusqlite): dataset tracking & player counts
├── events.rs         — Event system: UiEvent enum, EventSink trait, ChannelSink / ConsoleSink
└── tui/
    ├── mod.rs        — Terminal setup, main loop, keyboard routing
    ├── app.rs        — App state machine (Config / Dashboard screens, RunState)
    ├── config_screen.rs — Ratatui config form renderer
    └── dashboard.rs  — Ratatui live dashboard renderer
```

---

## ⚙️ How It Works

rookt runs a **two-pass streaming pipeline** over each monthly Lichess `.pgn.zst` file:

```
[Download .pgn.zst]
       │
       ▼
  ┌─ Pass 1 ──────────────────────────────────────────────────────┐
  │  Stream-decode the zstd file, parse headers only.             │
  │  Count valid games (matching event/time control/move count)   │
  │  per player. Build a "qualifying" set: players with           │
  │  ≥ min_monthly_games that month.                              │
  └───────────────────────────────────────────────────────────────┘
       │
       ▼
  ┌─ Pass 2 ──────────────────────────────────────────────────────┐
  │  Re-stream the same file, parse full PGN text.                │
  │  For each qualifying player's games, buffer the raw PGN in   │
  │  memory (up to 2 GB) and flush as a new zstd frame appended   │
  │  to their per-player file.                                    │
  └───────────────────────────────────────────────────────────────┘
       │
       ▼
  [Mark dataset done in SQLite → delete temp .zst]
       │
       ▼
  ┌─ Final Prune ─────────────────────────────────────────────────┐
  │  After all datasets are processed, remove any player whose    │
  │  total game count across all months is below min_total_games. │
  └───────────────────────────────────────────────────────────────┘
```

**Resumable by design** — the SQLite index records which monthly datasets have already been fully processed. If rookt is interrupted, it will skip completed months and resume from where it left off.

**No re-downloads** — if the `.zst.part` or completed `.zst` file already exists on disk, it will not be downloaded again.

---

## 💻 System Requirements

| Component | Minimum | Recommended |
|---|---|---|
| **Free disk space** | **300 GB** | 600 GB+ |
| **RAM** | 8 GB | 16 GB+ |
| **CPU** | Any modern x86\_64 | Fast single-core (pipeline is I/O-bound) |
| **OS** | Windows 10 / Linux / macOS | Any |
| **Internet** | Broadband | 100 Mbps+ (files are 10–50 GB each) |
| **Rust** | 1.85 (2024 edition) | Latest stable |

> [!WARNING]
> **Disk space is the main bottleneck.** A single Lichess monthly dump can be 30–40 GB compressed. Processing a full year at high player volume can easily consume 300–500 GB of output. Make sure your output directory is on a drive with sufficient headroom before starting. That's why the final output is compressed per player — to save space and make it manageable.

> [!NOTE]
> The write buffer defaults to **2 GB RAM**. On machines with less than 8 GB total RAM, consider reducing `write_buffer_max_bytes` in `config.rs` to avoid memory pressure during pass 2.

---

## 🐛 Reporting Bugs

Found a bug? Please open an issue with:

1. Your OS and Rust version (`rustc --version`)
2. The exact command / configuration you ran
3. The full error message or unexpected behaviour
4. If possible, a sample PGN snippet that reproduces the issue

👉 [Open an issue](https://github.com/andrewyernau/rookt/issues/new?labels=bug&template=bug_report.md)

---

## 🤝 Contributing

Contributions are welcome! Here's how to get started:

```bash
# Fork the repo, then:
git clone https://github.com/<your-username>/rookt.git
cd rookt
cargo test          # make sure everything passes
cargo clippy        # check for lints
```

Please keep pull requests focused — one feature or fix per PR. For significant changes, open an issue first to discuss the approach.

**Areas that could use help:**
- Multi-threaded pass 2 extraction
- Configurable dataset URL lists via a config file (TOML)
- Progress persistence / ETA estimation
- Additional filters (rating range, result, opening ECO)
- Linux / macOS packaging (`.deb`, Homebrew)

---

## 📄 License

This project is licensed under the **MIT License** — see the [LICENSE](LICENSE) file for details.

---

## 💬 Support

<div align="center">

| | |
|:---:|:---:|
| 💬 **Discord** | [Join the server](https://discord.gg/939UecsD95) |
| ☕ **Ko-fi** | [Buy me a coffee](https://ko-fi.com/andrewyernau) |
| 🐛 **Issues** | [GitHub Issues](https://github.com/andrewyernau/rookt/issues) |
| 👤 **Author** | [@andrewyernau](https://github.com/andrewyernau) |

</div>

<div align="center">
  <sub>Made with ♟️ and Rust by <a href="https://github.com/andrewyernau">andrewyernau</a></sub>
</div>
