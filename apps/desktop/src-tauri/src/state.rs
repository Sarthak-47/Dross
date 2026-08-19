//! Shared app state: the open repository and its index.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dross_core::authorship::{AuthorshipMap, BurstParams, Tag};
use dross_core::config::Config;
use dross_core::index::FingerprintIndex;

use crate::watcher::{RepoWatcher, WriteLog};

#[derive(Default)]
pub struct AppState {
    inner: Mutex<Inner>,
    write_log: Arc<WriteLog>,
}

#[derive(Default)]
struct Inner {
    repo_root: Option<PathBuf>,
    config: Config,
    /// User overrides made in the UI. Kept separate from detected tags so a
    /// correction survives the watcher recomputing its heuristic.
    overrides: AuthorshipMap,
    watcher: Option<RepoWatcher>,
}

impl AppState {
    pub fn set_repo(&self, root: PathBuf) {
        let mut inner = self.inner.lock().unwrap();
        inner.config = Config::load(&root);
        inner.overrides = AuthorshipMap::new();

        // A watcher for the previous repo would keep tagging writes that are
        // no longer relevant, so it is replaced rather than accumulated.
        self.write_log.clear();
        inner.watcher = RepoWatcher::start(root.clone(), Arc::clone(&self.write_log))
            .inspect_err(|e| log::warn!("file watcher unavailable: {e}"))
            .ok();

        inner.repo_root = Some(root);
    }

    /// True when the burst-write heuristic has a live source of events.
    pub fn watcher_active(&self) -> bool {
        self.inner.lock().unwrap().watcher.is_some()
    }

    pub fn repo_root(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().repo_root.clone()
    }

    pub fn require_repo(&self) -> Result<PathBuf, String> {
        self.repo_root()
            .ok_or_else(|| "no repository is open".to_string())
    }

    pub fn config(&self) -> Config {
        self.inner.lock().unwrap().config.clone()
    }

    pub fn set_config(&self, config: Config) {
        self.inner.lock().unwrap().config = config;
    }

    /// Detected tags merged with user overrides. Overrides are applied last so
    /// they win, which is what makes a mistagged hunk correctable rather than
    /// silently mis-scoped.
    pub fn authorship(&self) -> AuthorshipMap {
        let inner = self.inner.lock().unwrap();
        let mut map = self.write_log.authorship(BurstParams::default());
        for (path, ranges) in inner.overrides.iter() {
            for range in ranges {
                if let Tag::UserOverride(is_ai) = range.tag {
                    map.override_range(path.clone(), range.start_line, range.end_line, is_ai);
                }
            }
        }
        map
    }

    pub fn override_authorship(&self, path: PathBuf, start: usize, end: usize, is_ai: bool) {
        self.inner
            .lock()
            .unwrap()
            .overrides
            .override_range(path, start, end, is_ai);
    }

    /// Opens the repo's index, creating it if absent.
    pub fn open_index(&self) -> Result<FingerprintIndex, String> {
        let root = self.require_repo()?;
        FingerprintIndex::open(&Config::index_path(&root)).map_err(|e| e.to_string())
    }
}
