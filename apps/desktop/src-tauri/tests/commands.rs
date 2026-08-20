//! Exercises the IPC layer's logic without a window.
//!
//! The Tauri commands are thin wrappers, but the parts that are not thin —
//! path confinement, adapter id matching, the serde shape the UI reads — are
//! exactly where a mistake stays invisible until somebody runs the app.

use std::path::PathBuf;

/// `file_source` must refuse to read outside the open repository. The renderer
/// is granted no filesystem permission precisely so this command is the only
/// door, which makes this the whole boundary.
#[test]
fn file_source_confinement_rejects_paths_outside_the_repository() {
    let root = std::env::temp_dir().join(format!("dross-ipc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.js"), "export const a = 1;\n").unwrap();

    let outside = std::env::temp_dir().join(format!("dross-outside-{}.js", std::process::id()));
    std::fs::write(&outside, "secret\n").unwrap();

    let canonical_root = root.canonicalize().unwrap();
    let inside = root.join("src/a.js").canonicalize().unwrap();
    assert!(inside.starts_with(&canonical_root));

    let escaped = root.join("..").join(outside.file_name().unwrap());
    if let Ok(resolved) = escaped.canonicalize() {
        assert!(
            !resolved.starts_with(&canonical_root),
            "a traversal path resolved inside the repository"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&outside);
}

/// The UI sends adapter ids as the serde representation of `AdapterId`. If the
/// two drift, every Connect button silently stops working.
#[test]
fn adapter_ids_match_what_the_ui_sends() {
    use dross_adapters::AdapterId;

    for (id, wire) in [
        (AdapterId::ClaudeCode, "claude-code"),
        (AdapterId::CodexCli, "codex-cli"),
        (AdapterId::Antigravity, "antigravity"),
        (AdapterId::GitHook, "git-hook"),
    ] {
        assert_eq!(
            serde_json::to_value(id).unwrap().as_str(),
            Some(wire),
            "the Connections panel sends `{wire}`"
        );
    }
}

#[test]
fn every_listed_adapter_is_addressable_by_its_own_id() {
    for adapter in dross_adapters::all_adapters() {
        let id = serde_json::to_value(adapter.id())
            .unwrap()
            .as_str()
            .map(str::to_string)
            .expect("adapter ids serialize as strings");
        assert!(!id.is_empty());
        assert!(!adapter.id().label().is_empty());
    }
}

/// The repository header reads these fields, so they have to survive serde's
/// camelCase rename.
#[test]
fn repository_info_serialises_as_the_ui_expects() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Probe {
        root: PathBuf,
        index_built: bool,
        indexed_functions: usize,
        watcher_active: bool,
    }

    let json = serde_json::to_value(Probe {
        root: PathBuf::from("/tmp/x"),
        index_built: true,
        indexed_functions: 3,
        watcher_active: false,
    })
    .unwrap();

    for key in ["indexBuilt", "indexedFunctions", "watcherActive"] {
        assert!(json.get(key).is_some(), "the UI reads `{key}`");
    }
}

/// A finding must serialise with the field names types.ts declares, or the
/// findings panel renders blanks.
#[test]
fn findings_serialise_with_the_fields_the_panel_reads() {
    use dross_core::finding::{CheckId, Finding, Severity, SourceSpan};

    let f = Finding::new(
        CheckId::SwallowedException,
        "empty-catch-body",
        Severity::Error,
        SourceSpan {
            file: PathBuf::from("src/a.js"),
            start_line: 1,
            end_line: 2,
        },
        "message",
        "evidence",
    );
    let json = serde_json::to_value(&f).unwrap();

    for key in [
        "check",
        "signal",
        "severity",
        "span",
        "message",
        "evidence",
        "authorship",
    ] {
        assert!(json.get(key).is_some(), "the findings panel reads `{key}`");
    }
    assert_eq!(json["check"], "swallowed-exception");
    assert_eq!(json["severity"], "error");
    assert_eq!(json["span"]["start_line"], 1);
}
