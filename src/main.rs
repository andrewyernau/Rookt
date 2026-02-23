mod config;
mod database;
mod download;
mod events;
mod parser;
mod pipeline;
mod tui;
mod writer;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--headless") {
        // Headless mode: use default config and console output
        let config = config::Config::default_blitz_300();
        pipeline::run(&config)
    } else {
        // TUI mode: interactive config + dashboard
        tui::run()
    }
}
