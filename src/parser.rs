use anyhow::Result;
use std::io::BufRead;

/// Minimal game info extracted during pass 1 (counting).
pub struct GameInfo {
    pub event: String,
    pub white: String,
    pub black: String,
    pub time_control: String,
    pub half_move_count: u32,
}

/// Full game data including raw PGN text, for pass 2 (extraction).
pub struct Game {
    pub info: GameInfo,
    pub raw_pgn: String,
}

#[derive(PartialEq)]
enum State {
    BetweenGames,
    InHeaders,
    InMoves,
}

/// Streaming PGN parser. Reads from any `BufRead` source.
pub struct PgnParser<R> {
    reader: R,
    line_buf: String,
    pending_line: Option<String>,
}

impl<R: BufRead> PgnParser<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line_buf: String::with_capacity(4096),
            pending_line: None,
        }
    }

    /// Fill `self.line_buf` with the next line. Returns false at EOF.
    fn read_line(&mut self) -> Result<bool> {
        if let Some(pending) = self.pending_line.take() {
            self.line_buf = pending;
            return Ok(true);
        }
        self.line_buf.clear();
        let n = self.reader.read_line(&mut self.line_buf)?;
        Ok(n > 0)
    }

    /// Pass 1: Extract headers and half-move count only (no raw PGN stored).
    pub fn next_info(&mut self) -> Result<Option<GameInfo>> {
        let mut event = String::new();
        let mut white = String::new();
        let mut black = String::new();
        let mut time_control = String::new();
        let mut state = State::BetweenGames;
        let mut half_moves: u32 = 0;

        loop {
            if !self.read_line()? {
                return if state != State::BetweenGames {
                    Ok(Some(GameInfo {
                        event,
                        white,
                        black,
                        time_control,
                        half_move_count: half_moves,
                    }))
                } else {
                    Ok(None)
                };
            }

            let trimmed = self.line_buf.trim();

            if trimmed.is_empty() {
                match state {
                    State::InMoves => {
                        return Ok(Some(GameInfo {
                            event,
                            white,
                            black,
                            time_control,
                            half_move_count: half_moves,
                        }));
                    }
                    State::InHeaders => {
                        state = State::InMoves;
                    }
                    State::BetweenGames => {}
                }
                continue;
            }

            let is_header = trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.contains('"');

            match state {
                State::BetweenGames => {
                    if is_header {
                        state = State::InHeaders;
                        extract_header_into(trimmed, &mut event, &mut white, &mut black, &mut time_control);
                    }
                }
                State::InHeaders => {
                    if is_header {
                        extract_header_into(trimmed, &mut event, &mut white, &mut black, &mut time_control);
                    } else {
                        // No empty line between headers and moves — handle gracefully
                        state = State::InMoves;
                        half_moves += count_clk(trimmed);
                    }
                }
                State::InMoves => {
                    if is_header {
                        // Next game started without blank line separator
                        self.pending_line = Some(self.line_buf.clone());
                        return Ok(Some(GameInfo {
                            event,
                            white,
                            black,
                            time_control,
                            half_move_count: half_moves,
                        }));
                    }
                    half_moves += count_clk(trimmed);
                }
            }
        }
    }

    /// Pass 2: Extract full game including raw PGN text.
    pub fn next_game(&mut self) -> Result<Option<Game>> {
        let mut event = String::new();
        let mut white = String::new();
        let mut black = String::new();
        let mut time_control = String::new();
        let mut state = State::BetweenGames;
        let mut half_moves: u32 = 0;
        let mut raw = String::with_capacity(2048);

        loop {
            if !self.read_line()? {
                return if state != State::BetweenGames {
                    Ok(Some(Game {
                        info: GameInfo {
                            event,
                            white,
                            black,
                            time_control,
                            half_move_count: half_moves,
                        },
                        raw_pgn: raw,
                    }))
                } else {
                    Ok(None)
                };
            }

            // Normalize line ending
            let line = self.line_buf.trim_end_matches(|c| c == '\r' || c == '\n');
            let trimmed = line.trim();

            if trimmed.is_empty() {
                match state {
                    State::InMoves => {
                        raw.push('\n');
                        return Ok(Some(Game {
                            info: GameInfo {
                                event,
                                white,
                                black,
                                time_control,
                                half_move_count: half_moves,
                            },
                            raw_pgn: raw,
                        }));
                    }
                    State::InHeaders => {
                        state = State::InMoves;
                        raw.push('\n');
                    }
                    State::BetweenGames => {}
                }
                continue;
            }

            let is_header = trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.contains('"');

            match state {
                State::BetweenGames => {
                    if is_header {
                        state = State::InHeaders;
                        extract_header_into(trimmed, &mut event, &mut white, &mut black, &mut time_control);
                        raw.push_str(line);
                        raw.push('\n');
                    }
                }
                State::InHeaders => {
                    if is_header {
                        extract_header_into(trimmed, &mut event, &mut white, &mut black, &mut time_control);
                        raw.push_str(line);
                        raw.push('\n');
                    } else {
                        state = State::InMoves;
                        raw.push('\n'); // empty line between headers and moves
                        half_moves += count_clk(trimmed);
                        raw.push_str(line);
                        raw.push('\n');
                    }
                }
                State::InMoves => {
                    if is_header {
                        self.pending_line = Some(self.line_buf.clone());
                        return Ok(Some(Game {
                            info: GameInfo {
                                event,
                                white,
                                black,
                                time_control,
                                half_move_count: half_moves,
                            },
                            raw_pgn: raw,
                        }));
                    }
                    half_moves += count_clk(trimmed);
                    raw.push_str(line);
                    raw.push('\n');
                }
            }
        }
    }
}

/// Parse a PGN header line `[Key "Value"]` and update the relevant field.
fn extract_header_into(
    line: &str,
    event: &mut String,
    white: &mut String,
    black: &mut String,
    time_control: &mut String,
) {
    let inner = &line[1..line.len() - 1];
    let Some(space) = inner.find(' ') else { return };
    let key = &inner[..space];
    let rest = inner[space + 1..].trim();
    if rest.len() < 2 || !rest.starts_with('"') || !rest.ends_with('"') {
        return;
    }
    let value = &rest[1..rest.len() - 1];

    match key {
        "Event" => { event.clear(); event.push_str(value); }
        "White" => { white.clear(); white.push_str(value); }
        "Black" => { black.clear(); black.push_str(value); }
        "TimeControl" => { time_control.clear(); time_control.push_str(value); }
        _ => {}
    }
}

/// Count `[%clk` occurrences in a line (each = 1 half-move).
fn count_clk(line: &str) -> u32 {
    line.matches("[%clk").count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SAMPLE_PGN: &str = r#"[Event "Rated Blitz game"]
[Site "https://lichess.org/r0GRizwM"]
[Date "2025.08.01"]
[White "PlayerA"]
[Black "PlayerB"]
[Result "0-1"]
[TimeControl "300+0"]

1. e4 { [%clk 0:05:00] } 1... e5 { [%clk 0:05:00] } 2. Nf3 { [%clk 0:04:59] } 2... Nc6 { [%clk 0:04:59] } 0-1

[Event "Rated Rapid game"]
[Site "https://lichess.org/abc123"]
[Date "2025.08.02"]
[White "PlayerC"]
[Black "PlayerD"]
[Result "1-0"]
[TimeControl "900+0"]

1. d4 { [%clk 0:15:00] } 1... d5 { [%clk 0:15:00] } 1-0
"#;

    #[test]
    fn test_next_info_parses_two_games() {
        let cursor = Cursor::new(SAMPLE_PGN);
        let mut parser = PgnParser::new(cursor);

        let g1 = parser.next_info().unwrap().unwrap();
        assert_eq!(g1.event, "Rated Blitz game");
        assert_eq!(g1.white, "PlayerA");
        assert_eq!(g1.black, "PlayerB");
        assert_eq!(g1.time_control, "300+0");
        assert_eq!(g1.half_move_count, 4); // 4 [%clk annotations

        let g2 = parser.next_info().unwrap().unwrap();
        assert_eq!(g2.event, "Rated Rapid game");
        assert_eq!(g2.time_control, "900+0");
        assert_eq!(g2.half_move_count, 2);

        assert!(parser.next_info().unwrap().is_none());
    }

    #[test]
    fn test_next_game_captures_raw_pgn() {
        let cursor = Cursor::new(SAMPLE_PGN);
        let mut parser = PgnParser::new(cursor);

        let g1 = parser.next_game().unwrap().unwrap();
        assert_eq!(g1.info.white, "PlayerA");
        assert!(g1.raw_pgn.contains("[Event \"Rated Blitz game\"]"));
        assert!(g1.raw_pgn.contains("e4"));

        let g2 = parser.next_game().unwrap().unwrap();
        assert_eq!(g2.info.white, "PlayerC");

        assert!(parser.next_game().unwrap().is_none());
    }
}
