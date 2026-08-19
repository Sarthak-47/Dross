//! File watcher feeding burst-write authorship detection (spec section 5).
//!
//! The CLI only has commit trailers to work with. The app can watch the
//! working tree, which is where the burst-write heuristic gets its input:
//! an agent writing a feature produces many large writes in quick succession,
//! a human typing does not.
//!
//! The heuristic is deliberately conservative. Over-tagging costs one extra
//! check; under-tagging silently downgrades agent code to the lighter pass,
//! which is the failure users would never notice.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use dross_core::authorship::{BurstParams, WriteEvent, tag_from_write_bursts};
use dross_core::config::Config;
use dross_core::lang::Language;
use notify::{Event, EventKind, RecursiveMode, Watcher};

/// Rolling window of writes. Older events are dropped so a long session does
/// not grow without bound.
const MAX_EVENTS: usize = 2_000;

#[derive(Default)]
pub struct WriteLog {
    events: Mutex<Vec<WriteEvent>>,
}

impl WriteLog {
    pub fn record(&self, path: PathBuf, line_count: usize) {
        let mut events = self.events.lock().unwrap();
        if events.len() >= MAX_EVENTS {
            let overflow = events.len() - MAX_EVENTS + 1;
            events.drain(..overflow);
        }
        events.push(WriteEvent {
            path,
            timestamp_ms: now_ms(),
            start_line: 1,
            end_line: line_count.max(1),
        });
    }

    pub fn snapshot(&self) -> Vec<WriteEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }

    /// Authorship tags derived from the current write history.
    pub fn authorship(&self, params: BurstParams) -> dross_core::authorship::AuthorshipMap {
        tag_from_write_bursts(&self.snapshot(), params)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A running watcher. Dropping it stops the thread.
pub struct RepoWatcher {
    _watcher: notify::RecommendedWatcher,
}

impl RepoWatcher {
    /// Starts watching `root`, appending qualifying writes to `log`.
    pub fn start(root: PathBuf, log: Arc<WriteLog>) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        let watch_root = root.clone();
        std::thread::spawn(move || {
            let config = Config::default();
            for event in rx {
                let Ok(event) = event else { continue };
                if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    continue;
                }
                for path in event.paths {
                    // Ignore vendored trees and anything we cannot parse; a
                    // node_modules install would otherwise read as one huge
                    // agent burst.
                    if config.is_ignored(&path) || Language::from_path(&path).is_none() {
                        continue;
                    }
                    if !path.starts_with(&watch_root) {
                        continue;
                    }
                    let lines = std::fs::read_to_string(&path)
                        .map(|s| s.lines().count())
                        .unwrap_or(0);
                    if lines == 0 {
                        continue;
                    }
                    let relative = path
                        .strip_prefix(&watch_root)
                        .unwrap_or(&path)
                        .to_path_buf();
                    log.record(relative, lines);
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_caps_its_length() {
        let log = WriteLog::default();
        for i in 0..(MAX_EVENTS + 50) {
            log.record(PathBuf::from(format!("f{i}.ts")), 10);
        }
        assert!(log.snapshot().len() <= MAX_EVENTS);
    }

    #[test]
    fn a_single_small_write_is_not_tagged_as_agent_work() {
        let log = WriteLog::default();
        log.record(PathBuf::from("a.ts"), 3);
        let map = log.authorship(BurstParams::default());
        assert!(
            map.is_empty(),
            "one small edit must not be treated as an agent burst"
        );
    }
}
