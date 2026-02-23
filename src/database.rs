use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

/// SQLite database for tracking player game counts and processed datasets.
pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;

             CREATE TABLE IF NOT EXISTS players (
                 name TEXT PRIMARY KEY,
                 total_games INTEGER NOT NULL DEFAULT 0
             );

             CREATE TABLE IF NOT EXISTS monthly_counts (
                 player TEXT NOT NULL,
                 month TEXT NOT NULL,
                 games INTEGER NOT NULL,
                 PRIMARY KEY (player, month)
             );

             CREATE TABLE IF NOT EXISTS processed_datasets (
                 url TEXT PRIMARY KEY
             );

             CREATE INDEX IF NOT EXISTS idx_monthly_player
                 ON monthly_counts(player);
             CREATE INDEX IF NOT EXISTS idx_players_total
                 ON players(total_games);",
        )?;
        Ok(())
    }

    /// Check if a dataset URL has already been processed.
    pub fn is_dataset_processed(&self, url: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM processed_datasets WHERE url = ?1",
            [url],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Mark a dataset URL as processed.
    pub fn mark_dataset_processed(&self, url: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO processed_datasets (url) VALUES (?1)",
            [url],
        )?;
        Ok(())
    }

    /// Update player counts for a given month. Adds to total_games.
    pub fn update_player_counts(
        &mut self,
        month: &str,
        counts: &HashMap<String, u32>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;

        {
            let mut insert_monthly = tx.prepare(
                "INSERT OR REPLACE INTO monthly_counts (player, month, games) VALUES (?1, ?2, ?3)",
            )?;
            let mut upsert_player = tx.prepare(
                "INSERT INTO players (name, total_games) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET total_games = total_games + excluded.total_games",
            )?;

            for (player, &count) in counts {
                insert_monthly.execute(params![player, month, count as i64])?;
                upsert_player.execute(params![player, count as i64])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Get all player names with total games below the threshold.
    pub fn get_players_below_total(&self, min_total: u32) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM players WHERE total_games < ?1")?;
        let names = stmt
            .query_map([min_total], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(names)
    }

    /// Remove players (and their monthly data) with total games below threshold.
    pub fn remove_players_below_total(&mut self, min_total: u32) -> Result<usize> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM monthly_counts WHERE player IN (SELECT name FROM players WHERE total_games < ?1)",
            [min_total],
        )?;
        let deleted = tx.execute("DELETE FROM players WHERE total_games < ?1", [min_total])?;
        tx.commit()?;
        Ok(deleted)
    }

    /// Count players with total games >= threshold.
    pub fn get_total_qualifying_players(&self, min_total: u32) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM players WHERE total_games >= ?1",
            [min_total],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Count total tracked players.
    #[allow(dead_code)]
    pub fn get_total_players(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM players",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}
