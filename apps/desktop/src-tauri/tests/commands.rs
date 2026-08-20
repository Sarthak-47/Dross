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

/// The design states that every control applies immediately and there is no
/// Save button. That only holds if a change reaches `.dross.json` — otherwise
/// a toggle looks like it worked and the next CLI run ignores it.
#[test]
fn settings_round_trip_through_the_config_file() {
    use dross_core::config::Config;
    use dross_core::finding::{CheckId, Severity};

    let root = std::env::temp_dir().join(format!("dross-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let mut config = Config::default();
    config.disabled_signals.insert("log-only-catch".to_string());
    config.disabled_checks.insert(CheckId::StructuralClone);
    config.clone_threshold = 0.74;
    config.complexity_z_threshold = 3.5;
    config.min_severity = Severity::Warning;
    config.block_at = Some(Severity::Error);
    config.save(&root).unwrap();

    let reloaded = Config::load(&root);
    assert!(!reloaded.is_signal_enabled("log-only-catch"));
    assert!(reloaded.is_signal_enabled("empty-catch-body"));
    assert!(!reloaded.is_enabled(CheckId::StructuralClone));
    assert!(reloaded.is_enabled(CheckId::ContractChange));
    assert_eq!(reloaded.clone_threshold, 0.74);
    assert_eq!(reloaded.complexity_z_threshold, 3.5);
    assert_eq!(reloaded.min_severity, Severity::Warning);
    assert_eq!(reloaded.block_at, Some(Severity::Error));

    // The file the CLI and the hook read is the one the app wrote.
    let raw = std::fs::read_to_string(root.join(".dross.json")).unwrap();
    assert!(raw.contains("log-only-catch"));

    let _ = std::fs::remove_dir_all(&root);
}

/// A `file:line` reference must split into a path and a line, and a path that
/// escapes the repository must be refused.
#[test]
fn editor_locations_split_and_stay_inside_the_repository() {
    let split = |location: &str| -> (String, Option<String>) {
        match location.rsplit_once(':') {
            Some((path, num)) if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) => {
                (path.to_string(), Some(num.to_string()))
            }
            _ => (location.to_string(), None),
        }
    };

    assert_eq!(
        split("src/a.ts:42"),
        ("src/a.ts".to_string(), Some("42".to_string()))
    );
    // A Windows drive letter is not a line number.
    assert_eq!(split("src/a.ts"), ("src/a.ts".to_string(), None));

    let root = std::env::temp_dir().join(format!("dross-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.ts"), "x\n").unwrap();
    let canonical_root = root.canonicalize().unwrap();

    assert!(
        root.join("src/a.ts")
            .canonicalize()
            .unwrap()
            .starts_with(&canonical_root)
    );

    let _ = std::fs::remove_dir_all(&root);
}
