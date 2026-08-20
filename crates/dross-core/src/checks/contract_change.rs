//! Silent API-contract-change detector (spec section 5).
//!
//! Diffs function signatures independent of body changes. The point is to
//! catch breakage that isn't visible in the diff itself: the changed line is
//! in one file, the breakage is in every caller.

use std::collections::HashMap;

use crate::ast::{FunctionDef, Param};
use crate::finding::{CheckId, Finding, Severity, SourceSpan};

use super::{Check, CheckContext};

pub struct ContractChangeCheck;

impl Check for ContractChangeCheck {
    fn id(&self) -> CheckId {
        CheckId::ContractChange
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();

        // changed_files() rather than ctx.diffs, so ignored and generated
        // files are skipped here too.
        for file in ctx.changed_files() {
            // A test function has no external callers to break. Adding a
            // pytest fixture parameter, or making a test async, is not a
            // contract change — it was 24 of the 25 false positives this
            // check produced across the benchmark corpus.
            if super::tautological_test::is_non_production_path(&file.path) {
                continue;
            }
            let (Some(new_parsed), Some(old_parsed)) =
                (ctx.parsed(&file.path), ctx.parsed_old(file))
            else {
                continue;
            };

            // A name that appears more than once in a revision cannot be
            // paired confidently: `@overload` stubs, nested closures, and
            // shadowed helpers all share a qualified name. Collapsing them
            // into one entry made every variant get compared against an
            // arbitrary sibling, which produced thousands of bogus
            // "type changed" and "parameter added" findings across the
            // benchmark corpus. Ambiguity costs recall, never precision.
            let old_funcs = index_unique(old_parsed.functions());
            let new_funcs = index_unique(new_parsed.functions());

            for (name, new_func) in &new_funcs {
                let Some(old_func) = old_funcs.get(name) else {
                    continue;
                };
                findings.extend(compare(&file.path, name, old_func, new_func));
            }
        }

        findings
    }
}

/// Indexes functions by qualified name, dropping every name that occurs more
/// than once. Overload sets and shadowed definitions are unpairable, so they
/// are excluded rather than matched arbitrarily.
fn index_unique(functions: Vec<FunctionDef>) -> HashMap<String, FunctionDef> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for f in &functions {
        if let Some(name) = f.qualified_name() {
            *counts.entry(name).or_insert(0) += 1;
        }
    }
    functions
        .into_iter()
        .filter_map(|f| {
            let name = f.qualified_name()?;
            if counts.get(&name).copied().unwrap_or(0) > 1 {
                return None;
            }
            Some((name, f))
        })
        .collect()
}

fn compare(
    path: &std::path::Path,
    name: &str,
    old: &FunctionDef,
    new: &FunctionDef,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let span = SourceSpan {
        file: path.to_path_buf(),
        start_line: new.start_line,
        end_line: new.start_line,
    };

    let required = |ps: &[Param]| ps.iter().filter(|p| !p.optional && !p.variadic).count();
    let old_required = required(&old.params);
    let new_required = required(&new.params);

    // Narrowing: callers that used to compile now don't.
    if new_required > old_required {
        let added: Vec<&str> = new
            .params
            .iter()
            .skip(old.params.len())
            .filter(|p| !p.optional && !p.variadic)
            .map(|p| p.name.as_str())
            .collect();
        findings.push(Finding::new(
            CheckId::ContractChange,
            "required-parameter-added",
            Severity::Error,
            span.clone(),
            format!("`{name}` now requires {new_required} arguments, was {old_required}"),
            if added.is_empty() {
                "An existing optional parameter became required. Every existing call site \
                 that omitted it now breaks."
                    .to_string()
            } else {
                format!(
                    "New required parameter(s): {}. Every existing call site breaks unless \
                     updated in the same change.",
                    added.join(", ")
                )
            },
        ));
    }

    // Removal: fewer parameters accepted than before.
    if new.params.len() < old.params.len() {
        let removed: Vec<&str> = old
            .params
            .iter()
            .skip(new.params.len())
            .map(|p| p.name.as_str())
            .collect();
        findings.push(Finding::new(
            CheckId::ContractChange,
            "parameter-removed",
            Severity::Warning,
            span.clone(),
            format!("`{name}` dropped {} parameter(s)", removed.len()),
            format!(
                "Removed: {}. Call sites still passing these will silently pass ignored \
                 arguments in JS, or fail outright in typed code.",
                removed.join(", ")
            ),
        ));
    }

    // Parameters are aligned by name, not position. Positional comparison
    // reported a cascade of fake type changes whenever a parameter was
    // inserted: Flask adding a `ctx` argument to its methods produced 350
    // "parameter #N changed type" findings in one file, none of which
    // described what actually happened.
    let old_by_name: HashMap<&str, &Param> =
        old.params.iter().map(|p| (p.name.as_str(), p)).collect();

    for (i, n) in new.params.iter().enumerate() {
        let Some(o) = old_by_name.get(n.name.as_str()) else {
            // Present only in the new revision. A required addition is
            // already reported above as a count change.
            continue;
        };

        match (&o.ty, &n.ty) {
            (Some(ot), Some(nt)) if ot != nt => {
                findings.push(Finding::new(
                    CheckId::ContractChange,
                    "parameter-type-changed",
                    Severity::Warning,
                    span.clone(),
                    format!("`{name}` parameter `{}` changed type: `{ot}` -> `{nt}`", n.name),
                    format!(
                        "Parameter `{}` (position {}) was `{ot}` and is now `{nt}`. This is                          invisible at call sites until they are type-checked.",
                        n.name,
                        i + 1
                    ),
                ));
            }
            (Some(ot), None) => {
                findings.push(Finding::new(
                    CheckId::ContractChange,
                    "parameter-type-removed",
                    Severity::Info,
                    span.clone(),
                    format!("`{name}` parameter `{}` lost its type annotation", n.name),
                    format!("Was `{ot}`, now untyped — the contract is no longer enforced."),
                ));
            }
            _ => {}
        }

        if o.optional && !n.optional {
            findings.push(Finding::new(
                CheckId::ContractChange,
                "optional-parameter-became-required",
                Severity::Error,
                span.clone(),
                format!("`{name}` parameter `{}` is no longer optional", n.name),
                "Existing call sites that omitted this argument now break.".to_string(),
            ));
        }
    }

    // Return type change.
    if old.return_type != new.return_type
        && let (Some(ot), Some(nt)) = (&old.return_type, &new.return_type)
    {
        findings.push(Finding::new(
            CheckId::ContractChange,
            "return-type-changed",
            Severity::Warning,
            span.clone(),
            format!("`{name}` return type changed: `{ot}` -> `{nt}`"),
            format!(
                "Callers written against `{ot}` may mishandle `{nt}` without any \
                     diff-visible error at the call site."
            ),
        ));
    }

    // Sync -> async is a silent breaking change for every non-awaiting caller.
    if !old.is_async && new.is_async {
        findings.push(Finding::new(
            CheckId::ContractChange,
            "became-async",
            Severity::Error,
            span,
            format!("`{name}` became async"),
            "Callers that do not await now receive a Promise/coroutine instead of a value. \
             In JS this fails silently at runtime rather than at the call site."
                .to_string(),
        ));
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ParsedFile;
    use crate::lang::Language;

    fn compare_src(language: Language, old_src: &str, new_src: &str) -> Vec<Finding> {
        let old_file = ParsedFile::parse(language, old_src).unwrap();
        let new_file = ParsedFile::parse(language, new_src).unwrap();
        let old = old_file.functions().into_iter().next().unwrap();
        let new = new_file.functions().into_iter().next().unwrap();
        let name = new.qualified_name().unwrap();
        compare(std::path::Path::new("a.ts"), &name, &old, &new)
    }

    fn signals(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.signal.as_str()).collect()
    }

    #[test]
    fn flags_new_required_parameter() {
        let f = compare_src(
            Language::TypeScript,
            "function send(url: string) { return 1; }",
            "function send(url: string, retries: number) { return 1; }",
        );
        assert!(signals(&f).contains(&"required-parameter-added"));
    }

    #[test]
    fn ignores_new_optional_parameter() {
        let f = compare_src(
            Language::TypeScript,
            "function send(url: string) { return 1; }",
            "function send(url: string, retries?: number) { return 1; }",
        );
        assert!(f.is_empty(), "got {:?}", signals(&f));
    }

    #[test]
    fn flags_return_type_change() {
        let f = compare_src(
            Language::TypeScript,
            "function get(id: string): User { return u; }",
            "function get(id: string): User | null { return u; }",
        );
        assert!(signals(&f).contains(&"return-type-changed"));
    }

    #[test]
    fn flags_sync_to_async() {
        let f = compare_src(
            Language::TypeScript,
            "function load(id: string): User { return u; }",
            "async function load(id: string): User { return u; }",
        );
        assert!(signals(&f).contains(&"became-async"));
    }

    #[test]
    fn flags_optional_becoming_required() {
        let f = compare_src(
            Language::TypeScript,
            "function f(a: string, b?: number) { return 1; }",
            "function f(a: string, b: number) { return 1; }",
        );
        assert!(signals(&f).contains(&"optional-parameter-became-required"));
    }

    /// Regression: overload sets and shadowed helpers share a qualified name.
    /// Indexing them into a map collapsed each set to one arbitrary entry, so
    /// every sibling was compared against the wrong counterpart. Across the
    /// benchmark corpus this produced thousands of bogus findings — nearly
    /// half of everything the tool reported.
    #[test]
    fn duplicate_qualified_names_are_not_paired() {
        let overloads = "@overload
def instance_of(type: type[T]) -> int: ...
@overload
def instance_of(type: tuple[type[T]]) -> str: ...
";
        let changed = "@overload
def instance_of(type: tuple[type[T]]) -> str: ...
@overload
def instance_of(type: type[T]) -> int: ...
";
        let old_file = ParsedFile::parse(Language::Python, overloads).unwrap();
        let new_file = ParsedFile::parse(Language::Python, changed).unwrap();

        let old_index = index_unique(old_file.functions());
        let new_index = index_unique(new_file.functions());

        assert!(
            old_index.is_empty() && new_index.is_empty(),
            "a name declared twice must be excluded, not paired arbitrarily"
        );
    }

    #[test]
    fn a_uniquely_named_function_is_still_paired() {
        let index = index_unique(
            ParsedFile::parse(
                Language::Python,
                "def only(a):
    return a
",
            )
            .unwrap()
            .functions(),
        );
        assert!(index.contains_key("only"));
    }

    /// Regression: inserting a parameter shifted every later position, and
    /// positional comparison reported each shift as a type change. One Flask
    /// commit produced 350 such findings in a single file. Only the genuine
    /// addition should be reported.
    #[test]
    fn inserting_a_parameter_does_not_cascade_type_changes() {
        let f = compare_src(
            Language::Python,
            "def update(self, context):
    return context
",
            "def update(self, ctx, context):
    return context
",
        );
        let signals = signals(&f);
        assert!(
            signals.contains(&"required-parameter-added"),
            "the inserted parameter must still be reported: {signals:?}"
        );
        assert!(
            !signals.contains(&"parameter-type-changed"),
            "shifting a parameter must not be reported as a type change: {signals:?}"
        );
    }

    #[test]
    fn a_renamed_parameter_is_not_a_type_change() {
        let f = compare_src(
            Language::TypeScript,
            "function f(alpha: string): void {}",
            "function f(beta: string): void {}",
        );
        assert!(
            !signals(&f).contains(&"parameter-type-changed"),
            "a rename with an identical type must not report a type change"
        );
    }

    #[test]
    fn test_functions_are_not_contract_changes() {
        use crate::authorship::AuthorshipMap;
        use crate::config::Config;
        use crate::diff::{ChangeKind, FileDiff, Hunk};
        use crate::engine::Engine;
        use std::path::{Path, PathBuf};

        let before = "def test_thing(app):
    assert app
";
        let after = "def test_thing(app, client):
    assert app
";
        let make = |path: &str| FileDiff {
            path: PathBuf::from(path),
            old_path: None,
            kind: ChangeKind::Modified,
            language: Some(Language::Python),
            hunks: vec![Hunk {
                new_start: 1,
                new_lines: 2,
                old_start: 1,
                old_lines: 2,
            }],
            new_source: Some(after.to_string()),
            old_source: Some(before.to_string()),
        };

        let run = |path: &str| {
            let diffs = vec![make(path)];
            Engine::new(Config::default())
                .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
                .unwrap()
                .findings
                .into_iter()
                .filter(|f| f.check == CheckId::ContractChange)
                .count()
        };

        assert_eq!(
            run("tests/test_app.py"),
            0,
            "a test gained a fixture, not a contract change"
        );
        assert!(run("src/app.py") > 0, "library code must still be reported");
    }

    #[test]
    fn body_only_change_is_silent() {
        let f = compare_src(
            Language::TypeScript,
            "function f(a: string): number { return 1; }",
            "function f(a: string): number { return 2; }",
        );
        assert!(f.is_empty(), "got {:?}", signals(&f));
    }
}
