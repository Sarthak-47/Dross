//! Shared app state: the open repository and its index.

use std::path::PathBuf;
use std::sync::Mutex;

use dross_core::authorship::AuthorshipMap;
use dross_core::config::Config;
use dross_core::index::FingerprintIndex;

#[derive(Default)]
pub struct AppState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    repo_root: Option<PathBuf>,
    config: Config,
    /// Authorship tags accumulated for the open repo, including user
    /// overrides made in the UI.
    authorship: AuthorshipMap,
}

impl AppState {
    pub fn set_repo(&self, root: PathBuf) {
        let mut inner = self.inner.lock().unwrap();
        inner.config = Config::load(&root);
        inner.authorship = AuthorshipMap::new();
        inner.repo_root = Some(root);
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

    pub fn authorship(&self) -> AuthorshipMap {
        self.inner.lock().unwrap().authorship.clone()
    }

    pub fn override_authorship(&self, path: PathBuf, start: usize, end: usize, is_ai: bool) {
        self.inner
            .lock()
            .unwrap()
            .authorship
            .override_range(path, start, end, is_ai);
    }

    /// Opens the repo's index, creating it if absent.
    pub fn open_index(&self) -> Result<FingerprintIndex, String> {
        let root = self.require_repo()?;
        FingerprintIndex::open(&Config::index_path(&root)).map_err(|e| e.to_string())
    }
}
