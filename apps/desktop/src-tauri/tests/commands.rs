//! Exercises the IPC layer's logic without a window.
//!
//! The Tauri commands are thin wrappers, but the parts that are not thin —
//! path confinement, adapter id matching, the serde shape the UI reads — are
//! exactly where a mistake stays invisible until somebody runs the app.

use std::path::PathBuf;

/// Path confinement is the whole renderer boundary: the UI is granted no
/// filesystem permission of its own, so `resolve_in_repo` is the only door.
///
/// This calls the real function. An earlier version of this test re-implemented
/// the canonicalize-and-compare locally, which meant it would have kept passing
/// if the door itself were removed.
#[test]
fn path_confinement_rejects_paths_outside_the_repository() {
    use dross_desktop_lib::commands::resolve_in_repo;

    let root = std::env::temp_dir().join(format!("dross-ipc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.js"), "export const a = 1;\n").unwrap();

    let outside = std::env::temp_dir().join(format!("dross-outside-{}.js", std::process::id()));
    std::fs::write(&outside, "secret\n").unwrap();

    // The source pane passes a finding's `span.file` straight to file_source,
    // and spans are reported repo-relative — `source/loader.js`, not absolute.
    // Resolving one has to land on the file and read it.
    let resolved = resolve_in_repo(&root, "src/a.js").expect("a path inside the repo resolves");
    assert!(resolved.starts_with(root.canonicalize().unwrap()));
    assert_eq!(
        std::fs::read_to_string(&resolved).unwrap(),
        "export const a = 1;\n",
        "a repo-relative span must resolve to the file's real contents"
    );

    // Traversal out of the repository, spelled the way a renderer would have
    // to spell it.
    let escape = format!("../{}", outside.file_name().unwrap().to_string_lossy());
    let err = resolve_in_repo(&root, &escape).expect_err("a traversal path must be refused");
    assert!(err.contains("outside the open repository"), "{err}");

    // An absolute path handed straight in must not bypass the join either.
    let absolute = outside.display().to_string();
    assert!(
        resolve_in_repo(&root, &absolute).is_err(),
        "an absolute path outside the repository must be refused"
    );

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
///
/// Serialises the real `RepositoryInfo`. An earlier version declared a local
/// struct with the same shape, which tested that serde renames fields — never
/// in doubt — rather than that this type still has the fields types.ts declares.
#[test]
fn repository_info_serialises_as_the_ui_expects() {
    use dross_desktop_lib::commands::RepositoryInfo;

    let json = serde_json::to_value(RepositoryInfo {
        root: PathBuf::from("/tmp/x"),
        name: "x".into(),
        branch: Some("main".into()),
        index_built: true,
        indexed_functions: 3,
        baseline_samples: 412,
        watcher_active: false,
    })
    .unwrap();

    // Exactly the key set of the RepositoryInfo interface in types.ts. Asserted
    // as a whole set: an added backend field the UI never learns about is as
    // much a drift as a renamed one.
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "baselineSamples",
            "branch",
            "indexBuilt",
            "indexedFunctions",
            "name",
            "root",
            "watcherActive",
        ]
    );

    // A detached HEAD has to reach the UI as null, not as a missing key.
    let detached = serde_json::to_value(RepositoryInfo {
        root: PathBuf::from("/tmp/x"),
        name: "x".into(),
        branch: None,
        index_built: false,
        indexed_functions: 0,
        baseline_samples: 0,
        watcher_active: false,
    })
    .unwrap();
    assert!(detached["branch"].is_null());
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

/// The findings panel hands `open_in_editor` a `file:line` string built from a
/// finding's span, so the split has to survive the shapes those spans take.
///
/// Calls the real `split_location`; the previous version tested a closure
/// copied from it, which could not have detected the two diverging.
#[test]
fn editor_locations_split_into_a_path_and_a_line() {
    use dross_desktop_lib::commands::split_location;

    assert_eq!(
        split_location("src/a.ts:42"),
        ("src/a.ts", Some("42".into()))
    );

    // No line number: the whole string is the path.
    assert_eq!(split_location("src/a.ts"), ("src/a.ts", None));

    // A Windows absolute path ends in no digits, so the drive colon is not
    // mistaken for a line separator.
    assert_eq!(
        split_location(r"C:\repo\src\a.ts"),
        (r"C:\repo\src\a.ts", None)
    );

    // ...but one that *does* carry a line still splits on the last colon.
    assert_eq!(
        split_location(r"C:\repo\src\a.ts:7"),
        (r"C:\repo\src\a.ts", Some("7".into()))
    );

    // A trailing colon is not a line number.
    assert_eq!(split_location("src/a.ts:"), ("src/a.ts:", None));

    // Nor is a non-numeric suffix.
    assert_eq!(split_location("src/a.ts:beta"), ("src/a.ts:beta", None));
}
