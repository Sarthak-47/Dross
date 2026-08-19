//! IPC commands. Each is a thin wrapper: validate, delegate to dross-core,
//! map errors to strings the UI can render.

use std::path::PathBuf;

use dross_adapters::{all_adapters, detect_all, AdapterStatus};
use dross_core::config::Config;
use dross_core::diff::{DiffTarget, Repo};
use dross_core::engine::{Engine, Report};
use dross_core::index::RiskEntry;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::state::AppState;

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
pub struct RepositoryInfo {
    pub root: PathBuf,
    pub name: String,
    pub index_built: bool,
    pub indexed_functions: usize,
    pub baseline_samples: usize,
}

#[tauri::command]
pub fn open_repository(path: String, state: State<'_, AppState>) -> CmdResult<RepositoryInfo> {
    let repo = Repo::open(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let root = repo.workdir().map_err(|e| e.to_string())?.to_path_buf();
    state.set_repo(root);
    current_repository(state)
}

#[tauri::command]
pub fn current_repository(state: State<'_, AppState>) -> CmdResult<RepositoryInfo> {
    let root = state.require_repo()?;
    let index = state.open_index().ok();
    let indexed_functions = index.as_ref().and_then(|i| i.function_count().ok()).unwrap_or(0);
    let baseline_samples = index
        .as_ref()
        .and_then(|i| i.baseline_stats().ok().flatten())
        .map(|s| s.sample_count)
        .unwrap_or(0);

    Ok(RepositoryInfo {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.display().to_string()),
        index_built: indexed_functions > 0,
        indexed_functions,
        baseline_samples,
        root,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeArgs {
    /// "staged" or "worktree".
    pub target: String,
}

#[tauri::command]
pub fn analyze(args: AnalyzeArgs, state: State<'_, AppState>) -> CmdResult<Report> {
    let root = state.require_repo()?;
    let target = match args.target.as_str() {
        "staged" => DiffTarget::StagedVsHead,
        _ => DiffTarget::WorktreeVsHead,
    };

    let mut engine = Engine::new(state.config());
    if let Ok(index) = state.open_index() {
        engine = engine.with_index(index);
    }

    let report = engine
        .analyze_repo(&root, target, &state.authorship())
        .map_err(|e| e.to_string())?;

    // Record the run so the risk-history dashboard has a trend to draw.
    if let Ok(index) = state.open_index() {
        let _ = index.record_findings(None, &report.findings);
    }
    Ok(report)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub done: usize,
    pub total: usize,
    pub phase: String,
}

#[tauri::command]
pub fn build_index(
    window: tauri::Window,
    state: State<'_, AppState>,
) -> CmdResult<RepositoryInfo> {
    let root = state.require_repo()?;
    let index = state.open_index()?;
    let mut engine = Engine::new(state.config()).with_index(index);

    // Progress is emitted rather than blocking silently (spec section 4).
    engine
        .build_index(&root, |done, total| {
            let _ = window.emit(
                "dross://index-progress",
                IndexProgress {
                    done,
                    total,
                    phase: "fingerprints".to_string(),
                },
            );
        })
        .map_err(|e| e.to_string())?;

    let _ = window.emit(
        "dross://index-progress",
        IndexProgress {
            done: 0,
            total: 0,
            phase: "baseline".to_string(),
        },
    );
    let commits = state.config().baseline_commits;
    engine
        .build_complexity_baseline(&root, commits)
        .map_err(|e| e.to_string())?;

    current_repository(state)
}

#[tauri::command]
pub fn index_status(state: State<'_, AppState>) -> CmdResult<RepositoryInfo> {
    current_repository(state)
}

#[tauri::command]
pub fn list_connections(state: State<'_, AppState>) -> CmdResult<Vec<AdapterStatus>> {
    let root = state.require_repo()?;
    Ok(detect_all(&root))
}

#[tauri::command]
pub fn install_connection(id: String, state: State<'_, AppState>) -> CmdResult<Vec<AdapterStatus>> {
    let root = state.require_repo()?;
    let adapters = all_adapters();
    let adapter = adapters
        .iter()
        .find(|a| adapter_matches(a.id(), &id))
        .ok_or_else(|| format!("unknown integration `{id}`"))?;
    adapter.install(&root).map_err(|e| e.to_string())?;
    Ok(detect_all(&root))
}

#[tauri::command]
pub fn uninstall_connection(
    id: String,
    state: State<'_, AppState>,
) -> CmdResult<Vec<AdapterStatus>> {
    let root = state.require_repo()?;
    let adapters = all_adapters();
    let adapter = adapters
        .iter()
        .find(|a| adapter_matches(a.id(), &id))
        .ok_or_else(|| format!("unknown integration `{id}`"))?;
    adapter.uninstall(&root).map_err(|e| e.to_string())?;
    Ok(detect_all(&root))
}

fn adapter_matches(id: dross_adapters::AdapterId, name: &str) -> bool {
    serde_json::to_value(id)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .is_some_and(|s| s == name)
}

#[tauri::command]
pub fn risk_history(limit: usize, state: State<'_, AppState>) -> CmdResult<Vec<RiskEntry>> {
    let index = state.open_index()?;
    index.risk_history(limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> CmdResult<Config> {
    Ok(state.config())
}

#[tauri::command]
pub fn set_config(config: Config, state: State<'_, AppState>) -> CmdResult<Config> {
    let root = state.require_repo()?;
    config.save(&root).map_err(|e| e.to_string())?;
    state.set_config(config.clone());
    Ok(config)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorshipOverride {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub is_ai: bool,
}

/// Lets the user correct a mistagged hunk. Authorship detection is heuristic,
/// so the UI must be able to fix it rather than silently mis-scoping checks.
#[tauri::command]
pub fn override_authorship(args: AuthorshipOverride, state: State<'_, AppState>) -> CmdResult<()> {
    state.override_authorship(
        PathBuf::from(args.path),
        args.start_line,
        args.end_line,
        args.is_ai,
    );
    Ok(())
}

/// Reads a file's current contents for the diff view. Confined to the open
/// repository — a path outside it is rejected rather than read.
#[tauri::command]
pub fn file_source(path: String, state: State<'_, AppState>) -> CmdResult<String> {
    let root = state.require_repo()?;
    let candidate = root.join(&path);
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("cannot read {path}: {e}"))?;
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(&canonical_root) {
        return Err(format!("refusing to read {path}: outside the open repository"));
    }
    std::fs::read_to_string(&canonical).map_err(|e| e.to_string())
}
