//! CLI integration tests over a real temporary git repository.
//!
//! These cover the contract other tools depend on — exit codes, output shape,
//! and the hook gate — which unit tests on the engine cannot reach.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dross() -> &'static str {
    env!("CARGO_BIN_EXE_dross")
}

struct TempRepo {
    root: PathBuf,
}

impl TempRepo {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "dross-cli-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let repo = Self { root };
        repo.git(&["init", "-q"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "user.name", "test"]);
        repo.write("seed.txt", "seed\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-qm", "seed"]);
        repo
    }

    fn git(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("git must be available")
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(dross())
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("dross binary must run")
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const SWALLOWED: &str =
    "export function load(p) {\n  try {\n    return JSON.parse(read(p));\n  } catch (e) {}\n}\n";
const CLEAN: &str = "export function add(a, b) {\n  return a + b;\n}\n";

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn reports_a_clean_staged_change_with_exit_zero() {
    let repo = TempRepo::new("clean");
    repo.write("src/a.js", CLEAN);
    repo.git(&["add", "-A"]);

    let out = repo.run(&["check", "--staged"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("clean"), "got: {}", stdout(&out));
}

#[test]
fn finds_a_swallowed_exception_in_staged_changes() {
    let repo = TempRepo::new("finds");
    repo.write("src/a.js", SWALLOWED);
    repo.git(&["add", "-A"]);

    let out = repo.run(&["check", "--staged"]);
    let text = stdout(&out);
    assert!(text.contains("silently discarded"), "got: {text}");
    // Without --hook a finding must not change the exit code, so an advisory
    // run never breaks a caller that only wanted the report.
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn hook_mode_blocks_on_an_error_finding() {
    let repo = TempRepo::new("block");
    repo.write("src/a.js", SWALLOWED);
    repo.git(&["add", "-A"]);

    let out = repo.run(&["check", "--staged", "--hook"]);
    assert_eq!(out.status.code(), Some(1), "an error must block the commit");
}

#[test]
fn hook_mode_allows_a_clean_change() {
    let repo = TempRepo::new("allow");
    repo.write("src/a.js", CLEAN);
    repo.git(&["add", "-A"]);

    let out = repo.run(&["check", "--staged", "--hook"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn block_at_flag_raises_the_gate_above_the_finding() {
    let repo = TempRepo::new("gate");
    // A log-only catch is a warning, not an error.
    repo.write(
        "src/a.js",
        "export function load(p) {\n  try {\n    return read(p);\n  } catch (e) {\n    console.error(e);\n  }\n}\n",
    );
    repo.git(&["add", "-A"]);

    let strict = repo.run(&["check", "--staged", "--hook", "--block-at", "warning"]);
    assert_eq!(
        strict.status.code(),
        Some(1),
        "warning must block at warning"
    );

    let lenient = repo.run(&["check", "--staged", "--hook", "--block-at", "error"]);
    assert_eq!(
        lenient.status.code(),
        Some(0),
        "warning must not block when the gate is error"
    );
}

#[test]
fn json_output_is_valid_and_carries_the_findings() {
    let repo = TempRepo::new("json");
    repo.write("src/a.js", SWALLOWED);
    repo.git(&["add", "-A"]);

    let out = repo.run(&["check", "--staged", "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("output must be valid JSON");

    let findings = parsed["findings"].as_array().unwrap();
    assert!(!findings.is_empty());
    assert_eq!(findings[0]["check"], "swallowed-exception");
    assert!(parsed["risk_score"].as_u64().unwrap() > 0);
}

#[test]
fn compact_output_is_one_greppable_line_per_finding() {
    let repo = TempRepo::new("compact");
    repo.write("src/a.js", SWALLOWED);
    repo.git(&["add", "-A"]);

    let out = repo.run(&["check", "--staged", "--format", "compact"]);
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty());
    for line in lines {
        // file:line: severity: message [check/signal]
        assert!(line.contains(".js:"), "missing location in: {line}");
        assert!(line.contains('['), "missing check tag in: {line}");
    }
}

#[test]
fn analysis_is_deterministic_across_runs() {
    let repo = TempRepo::new("determinism");
    repo.write("src/a.js", SWALLOWED);
    repo.write("src/b.js", CLEAN);
    repo.git(&["add", "-A"]);

    let strip_timing = |out: &Output| {
        let mut v: serde_json::Value = serde_json::from_str(&stdout(out)).unwrap();
        v.as_object_mut().unwrap().remove("duration_ms");
        v
    };

    let first = repo.run(&["check", "--staged", "--format", "json"]);
    let second = repo.run(&["check", "--staged", "--format", "json"]);
    assert_eq!(
        strip_timing(&first),
        strip_timing(&second),
        "identical input must produce identical findings"
    );
}

#[test]
fn init_writes_a_config_file() {
    let repo = TempRepo::new("init");
    let out = repo.run(&["init"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(repo.path().join(".dross.json").exists());
}

#[test]
fn connections_install_writes_a_hook_and_ignores_the_index() {
    let repo = TempRepo::new("connect");

    let out = repo.run(&["connections", "install", "git"]);
    assert_eq!(out.status.code(), Some(0));

    let hook = std::fs::read_to_string(repo.path().join(".git/hooks/pre-commit")).unwrap();
    assert!(hook.contains("dross-managed"));
    // A hook that skips silently when the binary is missing is the failure
    // this assertion exists to prevent.
    assert!(hook.contains("did NOT run"));

    let ignore = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert!(
        ignore.contains(".dross/"),
        "index must be excluded from git"
    );

    let listed = repo.run(&["connections"]);
    assert!(stdout(&listed).contains("connected"));
}

#[test]
fn errors_outside_a_repository_instead_of_panicking() {
    let dir = std::env::temp_dir().join(format!("dross-norepo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let out = Command::new(dross())
        .args(["check", "--staged"])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "usage errors exit 2, not 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("repository"), "got: {stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}
