//! Launch history and frecency ranking.
//!
//! Every launched target is appended to `~/.local/share/aerofi/history.json`
//! (a flat JSON array of [`ExecutionRecord`]s). Frecency combines recency
//! decay with raw frequency: each launch of a target contributes points
//! depending on how long ago it happened, and the points are summed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;
use serde::{Deserialize, Serialize};

/// A single launch, persisted in `history.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Stable identifier of the launched target (path to the `.app`
    /// bundle or the script on disk).
    pub target_identifier: SharedString,
    /// Seconds since UNIX_EPOCH.
    pub timestamp: u64,
}

/// Launch history backing the frecency ranking.
pub struct History {
    records: Vec<ExecutionRecord>,
    path: PathBuf,
    /// Lazily computed frecency map, invalidated on `record_launch`.
    frecency_cache: RefCell<Option<HashMap<String, u32>>>,
}

impl History {
    /// Load the history from `~/.local/share/aerofi/history.json`,
    /// creating the directory and the file on first run. A missing file
    /// is not an error; an unreadable or malformed one falls back to an
    /// empty history (with a warning).
    pub fn load() -> Self {
        Self::from_path(history_path())
    }

    /// Load from an explicit path (no `home_dir` involved), so the
    /// first-run file creation and the error paths are testable.
    pub(crate) fn from_path(path: PathBuf) -> Self {
        if !path.is_file() {
            let history = Self {
                records: Vec::new(),
                path,
                frecency_cache: RefCell::new(None),
            };
            history.save();
            return history;
        }
        let records = match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<Vec<ExecutionRecord>>(&contents) {
                Ok(records) => records,
                Err(err) => {
                    eprintln!(
                        "aerofi: warning: failed to parse {}: {err}; starting with empty history",
                        path.display()
                    );
                    Vec::new()
                }
            },
            Err(err) => {
                eprintln!(
                    "aerofi: warning: failed to read {}: {err}; starting with empty history",
                    path.display()
                );
                Vec::new()
            }
        };
        Self {
            records,
            path,
            frecency_cache: RefCell::new(None),
        }
    }

    /// Append a launch of `target_identifier` with the current timestamp
    /// and persist the history to disk. Keep at most 2000 records.
    pub fn record_launch(&mut self, target_identifier: &str) {
        self.records.push(ExecutionRecord {
            target_identifier: SharedString::from(target_identifier),
            timestamp: now_secs(),
        });
        if self.records.len() > 2000 {
            // drain keeps the existing allocation; split_off would throw it away.
            let drop_count = self.records.len() - 2000;
            self.records.drain(0..drop_count);
        }
        *self.frecency_cache.borrow_mut() = None;
        self.save();
    }

    /// Frecency score: the sum over all launches of the target of the
    /// points its recency earns — 100 for a launch < 4 hours old, 80 for
    /// < 1 day, 40 for < 7 days, 10 for anything older.
    #[allow(dead_code)]
    pub fn calculate_frecency(&self, target_identifier: &str) -> u32 {
        let now = now_secs();
        self.records
            .iter()
            .filter(|r| r.target_identifier == target_identifier)
            .map(|r| recency_points(now.saturating_sub(r.timestamp)))
            .sum()
    }

    /// Calculate the frecency points for all recorded targets in a single pass.
    /// Returns a cached map of target identifier to its total frecency score.
    /// The cache is invalidated when `record_launch` is called.
    pub fn calculate_frecency_map(&self) -> std::collections::HashMap<String, u32> {
        if let Some(cached) = self.frecency_cache.borrow().as_ref() {
            return cached.clone();
        }
        let now = now_secs();
        let mut map = std::collections::HashMap::new();
        for r in &self.records {
            let pts = recency_points(now.saturating_sub(r.timestamp));
            *map.entry(r.target_identifier.to_string()).or_insert(0) += pts;
        }
        *self.frecency_cache.borrow_mut() = Some(map.clone());
        map
    }

    /// Persist the current records to `path` (best effort: failures are
    /// reported to stderr and never fatal — the daemon must not crash).
    fn save(&self) {
        if let Some(parent) = self.path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            eprintln!(
                "aerofi: warning: failed to create {}: {err}",
                parent.display()
            );
            return;
        }
        match serde_json::to_string_pretty(&self.records) {
            Ok(json) => {
                if let Err(err) = fs::write(&self.path, json) {
                    eprintln!(
                        "aerofi: warning: failed to write {}: {err}",
                        self.path.display()
                    );
                }
            }
            Err(err) => eprintln!("aerofi: warning: failed to serialize history: {err}"),
        }
    }
}

/// Points a single launch earns, by age in seconds.
fn recency_points(age_secs: u64) -> u32 {
    const FOUR_HOURS: u64 = 4 * 3600;
    const ONE_DAY: u64 = 24 * 3600;
    const SEVEN_DAYS: u64 = 7 * 24 * 3600;
    match age_secs {
        a if a < FOUR_HOURS => 100,
        a if a < ONE_DAY => 80,
        a if a < SEVEN_DAYS => 40,
        _ => 10,
    }
}

/// Seconds since UNIX_EPOCH (0 if the clock is before the epoch).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `~/.local/share/aerofi/history.json`.
fn history_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".local")
        .join("share")
        .join("aerofi")
        .join("history.json")
}

#[cfg(test)]
impl History {
    /// Test-only constructor: no disk access.
    pub fn test_new(path: PathBuf, records: Vec<ExecutionRecord>) -> Self {
        Self {
            records,
            path,
            frecency_cache: RefCell::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(identifier: &str, secs_ago: u64) -> ExecutionRecord {
        ExecutionRecord {
            target_identifier: identifier.into(),
            timestamp: now_secs().saturating_sub(secs_ago),
        }
    }

    fn history(records: Vec<ExecutionRecord>) -> History {
        History::test_new(PathBuf::new(), records)
    }

    #[test]
    fn frecency_sums_recency_tiers() {
        let history = history(vec![
            record("t", 60),         // < 4h -> 100
            record("t", 3 * 3600),   // < 4h -> 100
            record("t", 20 * 3600),  // < 1d -> 80
            record("t", 3 * 86400),  // < 7d -> 40
            record("t", 30 * 86400), // older -> 10
            record("other", 60),
        ]);
        assert_eq!(history.calculate_frecency("t"), 330);
        assert_eq!(history.calculate_frecency("other"), 100);
        assert_eq!(history.calculate_frecency("unknown"), 0);
    }

    #[test]
    fn record_launch_appends_and_persists_roundtrip() {
        let path = std::env::temp_dir().join(format!(
            "aerofi-history-{}-{}.json",
            std::process::id(),
            now_secs()
        ));
        let mut history = History::test_new(path.clone(), Vec::new());
        history.record_launch("/tmp/script.sh");
        assert_eq!(history.records.len(), 1);

        let reloaded = History::from_path(path.clone());
        assert_eq!(reloaded.records, history.records);
        let _ = fs::remove_file(&path);
    }
}
