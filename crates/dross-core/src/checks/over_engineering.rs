//! Over-engineering / needless-complexity detector (spec section 5a).
//!
//! All six signals from the spec are implemented. Three of them
//! (single-implementation abstraction, unused generality, excess indirection)
//! depend on repo-wide name resolution, which `symbols` does syntactically
//! rather than semantically. Each of those consults `SymbolTable::is_ambiguous`
//! and stays silent on ambiguous names, trading recall for precision.

use tree_sitter::Node;

use crate::ast::{ParsedFile, walk};
use crate::finding::{CheckId, Finding, Severity, SourceSpan};
use crate::metrics;
use crate::symbols::{DeclKind, SymbolTable};

use super::{Check, CheckContext};

/// Z-score past which complexity-per-line counts as an outlier. 2.5 is ~1% of
/// a normal distribution; the benchmark run should tune this.
pub const OUTLIER_Z_THRESHOLD: f64 = 2.5;

pub struct OverEngineeringCheck;

impl Check for OverEngineeringCheck {
    fn id(&self) -> CheckId {
        CheckId::OverEngineering
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        let symbols = ctx.symbols();

        for file in ctx.changed_files() {
            let Some(parsed) = ctx.parsed(&file.path) else {
                continue;
            };
            findings.extend(pass_through_wrappers(parsed, file, &symbols));
            findings.extend(single_implementation_abstractions(parsed, file, &symbols));
            findings.extend(overkill_patterns(parsed, file));
            findings.extend(unused_generality(parsed, file, &symbols));
            findings.extend(excess_indirection(parsed, file, &symbols));
        }

        findings.extend(complexity_outlier(ctx));
        findings
    }
}

// --- Signal: pass-through wrapper ---------------------------------------

fn pass_through_wrappers(
    parsed: &ParsedFile,
    file: &crate::diff::FileDiff,
    symbols: &SymbolTable,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for func in parsed.functions() {
        if !file.touches_range(func.start_line, func.end_line) {
            continue;
        }
        let Some(name) = func.name.as_deref() else {
            continue;
        };
        let Some(node) = parsed
            .root()
            .descendant_for_byte_range(func.start_byte, func.end_byte)
        else {
            continue;
        };
        let Some(inner) = forwarding_target(parsed, node) else {
            continue;
        };
        // Called from exactly one site: the wrapper adds indirection with no
        // reuse to justify it.
        let sites = symbols.call_site_count(name);
        if sites > 1 || symbols.is_ambiguous(name) {
            continue;
        }
        out.push(Finding::new(
            CheckId::OverEngineering,
            "pass-through-wrapper",
            Severity::Info,
            SourceSpan {
                file: file.path.clone(),
                start_line: func.start_line,
                end_line: func.end_line,
            },
            format!("`{name}` only forwards to `{inner}`"),
            format!(
                "The body is a single call to `{inner}` with no added logic, and `{name}` is \
                 called from {sites} site(s). The indirection can be removed by calling \
                 `{inner}` directly."
            ),
        ));
    }
    out
}

/// Returns the callee name if the body is exactly `return f(...)` / `f(...)`.
fn forwarding_target(file: &ParsedFile, func: Node<'_>) -> Option<String> {
    let body = func.child_by_field_name("body")?;
    let mut cursor = body.walk();
    let statements: Vec<Node<'_>> = body
        .named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect();
    if statements.len() != 1 {
        return None;
    }
    let stmt = statements[0];
    let expr = match stmt.kind() {
        "return_statement" => {
            let mut c = stmt.walk();
            stmt.named_children(&mut c)
                .find(|n| n.kind() != "comment")?
        }
        "expression_statement" => {
            let mut c = stmt.walk();
            stmt.named_children(&mut c)
                .find(|n| n.kind() != "comment")?
        }
        _ => return None,
    };
    // Unwrap `await f(...)`.
    let call = if expr.kind() == "await_expression" {
        let mut c = expr.walk();
        expr.named_children(&mut c).next()?
    } else {
        expr
    };
    if !matches!(call.kind(), "call_expression" | "call") {
        return None;
    }
    let callee = call.child_by_field_name("function")?;
    let text = file.text(callee).trim();
    let leaf = text.rsplit('.').next().unwrap_or(text);
    if leaf.is_empty() {
        return None;
    }
    Some(leaf.to_string())
}

// --- Signal: single-implementation abstraction ---------------------------

fn single_implementation_abstractions(
    parsed: &ParsedFile,
    file: &crate::diff::FileDiff,
    symbols: &SymbolTable,
) -> Vec<Finding> {
    let mut out = Vec::new();
    walk(parsed.root(), |node| {
        let kind = match node.kind() {
            "interface_declaration" => DeclKind::Interface,
            "abstract_class_declaration" => DeclKind::AbstractClass,
            _ => return,
        };
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = parsed.text(name_node).to_string();
        let (start_line, end_line) = parsed.line_span(node);
        if !file.touches_range(start_line, end_line) {
            return;
        }
        if symbols.is_ambiguous(&name) {
            return;
        }
        let implementors = symbols.subtypes(&name);
        if implementors.len() != 1 {
            return;
        }
        // A test double counts as a second implementation in spirit, so a name
        // that looks like one keeps the abstraction justified.
        let only = implementors[0];
        if looks_like_test_double(only) {
            return;
        }
        let label = if kind == DeclKind::Interface {
            "interface"
        } else {
            "abstract class"
        };
        out.push(Finding::new(
            CheckId::OverEngineering,
            "single-implementation-abstraction",
            Severity::Info,
            SourceSpan {
                file: file.path.clone(),
                start_line,
                end_line,
            },
            format!("`{name}` has exactly one implementation (`{only}`)"),
            format!(
                "This {label} is implemented only by `{only}` repo-wide, with no test double. \
                 Until a second implementation exists, `{only}` can be used directly."
            ),
        ));
    });
    out
}

fn looks_like_test_double(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "mock", "fake", "stub", "spy", "dummy", "test", "inmemory", "noop",
    ]
    .iter()
    .any(|n| lower.contains(n))
}

// --- Signal: overkill design pattern -------------------------------------

fn overkill_patterns(parsed: &ParsedFile, file: &crate::diff::FileDiff) -> Vec<Finding> {
    let mut out = Vec::new();
    for func in parsed.functions() {
        if !file.touches_range(func.start_line, func.end_line) {
            continue;
        }
        let Some(name) = func.name.as_deref() else {
            continue;
        };
        if !is_factory_name(name) {
            continue;
        }
        let Some(node) = parsed
            .root()
            .descendant_for_byte_range(func.start_byte, func.end_byte)
        else {
            continue;
        };
        let branches = dispatch_branch_count(parsed, node);
        if branches != 1 {
            continue;
        }
        out.push(Finding::new(
            CheckId::OverEngineering,
            "overkill-design-pattern",
            Severity::Info,
            SourceSpan {
                file: file.path.clone(),
                start_line: func.start_line,
                end_line: func.end_line,
            },
            format!("`{name}` dispatches to exactly one variant"),
            format!(
                "`{name}` is shaped like a factory/strategy selector but only one concrete \
                 branch is registered, so the dispatch never chooses anything."
            ),
        ));
    }
    out
}

fn is_factory_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "factory", "create", "make", "build", "resolve", "strategy", "provider",
    ]
    .iter()
    .any(|n| lower.contains(n))
}

/// Counts the distinct values a dispatcher can return: switch cases, if/elif
/// arms, or entries in a returned lookup object.
fn dispatch_branch_count(file: &ParsedFile, func: Node<'_>) -> usize {
    let mut cases = 0;
    let mut object_entries = 0;
    walk(func, |n| match n.kind() {
        "switch_case" | "case_statement" => cases += 1,
        "if_statement" | "elif_clause" => cases += 1,
        "pair" => object_entries += 1,
        _ => {}
    });
    // A `return new Foo()` with no branching is not a dispatcher at all; treat
    // a single unconditional construction as one branch.
    let total = cases.max(object_entries);
    if total == 0 {
        let mut constructions = 0;
        walk(func, |n| {
            if matches!(n.kind(), "new_expression") {
                constructions += 1;
            }
        });
        // Ignore trivially small helper factories with no construction at all.
        let _ = file;
        return constructions;
    }
    total
}

// --- Signal: unused generality -------------------------------------------

fn unused_generality(
    parsed: &ParsedFile,
    file: &crate::diff::FileDiff,
    symbols: &SymbolTable,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for func in parsed.functions() {
        if !file.touches_range(func.start_line, func.end_line) {
            continue;
        }
        let Some(name) = func.name.as_deref() else {
            continue;
        };
        if symbols.is_ambiguous(name) {
            continue;
        }
        let sites = symbols.call_site_count(name);
        // Needs enough call sites for "only ever one value" to be meaningful.
        if sites < 3 {
            continue;
        }
        for (i, param) in func.params.iter().enumerate() {
            if !param.optional && !is_config_flag(param) {
                continue;
            }
            let distinct = symbols.distinct_arguments_at(name, i);
            if distinct.len() != 1 {
                continue;
            }
            out.push(Finding::new(
                CheckId::OverEngineering,
                "unused-generality",
                Severity::Info,
                SourceSpan {
                    file: file.path.clone(),
                    start_line: func.start_line,
                    end_line: func.end_line,
                },
                format!(
                    "`{name}` parameter `{}` is always `{}`",
                    param.name, distinct[0]
                ),
                format!(
                    "Across all {sites} call sites, `{}` is only ever passed `{}`. The \
                     parameter adds a configuration surface nothing uses.",
                    param.name, distinct[0]
                ),
            ));
        }
    }
    out
}

fn is_config_flag(param: &crate::ast::Param) -> bool {
    let ty = param.ty.as_deref().unwrap_or("");
    ty == "boolean" || ty == "bool" || param.name.starts_with("is") || param.name.starts_with("use")
}

// --- Signal: excess indirection depth ------------------------------------

/// Chain length past which a low-branching operation looks over-routed.
const INDIRECTION_DEPTH_THRESHOLD: usize = 4;

fn excess_indirection(
    parsed: &ParsedFile,
    file: &crate::diff::FileDiff,
    symbols: &SymbolTable,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for func in parsed.functions() {
        if !file.touches_range(func.start_line, func.end_line) {
            continue;
        }
        let Some(name) = func.name.as_deref() else {
            continue;
        };
        if symbols.is_ambiguous(name) {
            continue;
        }
        let Some(node) = parsed
            .root()
            .descendant_for_byte_range(func.start_byte, func.end_byte)
        else {
            continue;
        };
        let m = metrics::node_metrics(node);
        // Only "simple" operations qualify — a complex function earns its depth.
        if m.cyclomatic > 2 {
            continue;
        }
        let depth = forwarding_chain_depth(parsed, symbols, name, 0, &mut Vec::new());
        if depth < INDIRECTION_DEPTH_THRESHOLD {
            continue;
        }
        out.push(Finding::new(
            CheckId::OverEngineering,
            "excess-indirection-depth",
            Severity::Info,
            SourceSpan {
                file: file.path.clone(),
                start_line: func.start_line,
                end_line: func.end_line,
            },
            format!("`{name}` routes a simple operation through {depth} forwarding hops"),
            format!(
                "Cyclomatic complexity is {}, but the call chain from `{name}` to the actual \
                 logic is {depth} levels deep. Each hop adds a file to read without adding \
                 behaviour.",
                m.cyclomatic
            ),
        ));
    }
    out
}

/// Follows single-call forwarding chains. Depth-limited and cycle-guarded.
fn forwarding_chain_depth(
    parsed: &ParsedFile,
    symbols: &SymbolTable,
    name: &str,
    depth: usize,
    seen: &mut Vec<String>,
) -> usize {
    if depth > 12 || seen.iter().any(|s| s == name) {
        return depth;
    }
    seen.push(name.to_string());

    let Some(func) = parsed
        .functions()
        .into_iter()
        .find(|f| f.name.as_deref() == Some(name))
    else {
        return depth;
    };
    let Some(node) = parsed
        .root()
        .descendant_for_byte_range(func.start_byte, func.end_byte)
    else {
        return depth;
    };
    match forwarding_target(parsed, node) {
        Some(next) if !symbols.is_ambiguous(&next) => {
            forwarding_chain_depth(parsed, symbols, &next, depth + 1, seen)
        }
        _ => depth,
    }
}

// --- Signal: complexity-to-problem-size outlier --------------------------

fn complexity_outlier(ctx: &CheckContext<'_>) -> Vec<Finding> {
    let Some(index) = ctx.index else {
        return Vec::new();
    };
    let Ok(Some(stats)) = index.baseline_stats() else {
        // Fewer than 30 baseline samples: stay silent rather than fire on noise.
        return Vec::new();
    };

    let mut total_complexity = 0usize;
    let mut total_lines = 0usize;
    let mut anchor: Option<SourceSpan> = None;

    for file in ctx.changed_files() {
        let Some(parsed) = ctx.parsed(&file.path) else {
            continue;
        };
        total_lines += file.changed_line_count();
        for func in parsed.functions() {
            if !file.touches_range(func.start_line, func.end_line) {
                continue;
            }
            let m = metrics::function_metrics(parsed, &func);
            total_complexity += m.cyclomatic + m.node_count / 10;
            if anchor.is_none() {
                anchor = Some(SourceSpan {
                    file: file.path.clone(),
                    start_line: func.start_line,
                    end_line: func.end_line,
                });
            }
        }
    }

    if total_lines == 0 || anchor.is_none() {
        return Vec::new();
    }
    let ratio = total_complexity as f64 / total_lines as f64;
    let z = stats.z_score(ratio);
    if z < ctx.config.complexity_z_threshold {
        return Vec::new();
    }

    vec![Finding::new(
        CheckId::OverEngineering,
        "complexity-to-problem-size-outlier",
        Severity::Warning,
        anchor.unwrap(),
        format!("This change is {z:.1} standard deviations more complex than this repo's norm"),
        format!(
            "Complexity per changed line is {ratio:.2} against a repo baseline of {:.2} \
             (sd {:.2}, n={}). The baseline comes from this repository's own history, so \
             this is measured against how this codebase normally solves similarly-sized \
             problems.",
            stats.mean, stats.std_dev, stats.sample_count
        ),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{ChangeKind, FileDiff, Hunk};
    use crate::lang::Language;
    use std::path::PathBuf;

    fn whole_file_diff(path: &str, source: &str, language: Language) -> FileDiff {
        let lines = source.lines().count().max(1);
        FileDiff {
            path: PathBuf::from(path),
            old_path: None,
            kind: ChangeKind::Added,
            language: Some(language),
            hunks: vec![Hunk {
                new_start: 1,
                new_lines: lines,
                old_start: 0,
                old_lines: 0,
            }],
            new_source: Some(source.to_string()),
            old_source: None,
        }
    }

    #[test]
    fn flags_single_call_pass_through_wrapper() {
        let src = "function fetchUser(id) { return getUser(id); }\nfetchUser(1);\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        let f = pass_through_wrappers(&parsed, &diff, &symbols);
        assert_eq!(f.len(), 1, "got {f:?}");
        assert_eq!(f[0].signal, "pass-through-wrapper");
    }

    #[test]
    fn does_not_flag_wrapper_with_multiple_callers() {
        let src = "function fetchUser(id) { return getUser(id); }\nfetchUser(1);\nfetchUser(2);\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        assert!(pass_through_wrappers(&parsed, &diff, &symbols).is_empty());
    }

    #[test]
    fn does_not_flag_wrapper_that_adds_logic() {
        let src = "function fetchUser(id) { validate(id); return getUser(id); }\nfetchUser(1);\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        assert!(pass_through_wrappers(&parsed, &diff, &symbols).is_empty());
    }

    #[test]
    fn flags_interface_with_one_implementation() {
        let src = "interface Store { get(k: string): string; }\nclass RedisStore implements Store { get(k: string) { return k; } }\n";
        let parsed = ParsedFile::parse(Language::TypeScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.ts"), &parsed);
        let diff = whole_file_diff("a.ts", src, Language::TypeScript);
        let f = single_implementation_abstractions(&parsed, &diff, &symbols);
        assert_eq!(f.len(), 1, "got {f:?}");
    }

    #[test]
    fn does_not_flag_interface_with_a_test_double() {
        let src = "interface Store { get(k: string): string; }\nclass MockStore implements Store { get(k: string) { return k; } }\n";
        let parsed = ParsedFile::parse(Language::TypeScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.ts"), &parsed);
        let diff = whole_file_diff("a.ts", src, Language::TypeScript);
        assert!(single_implementation_abstractions(&parsed, &diff, &symbols).is_empty());
    }

    #[test]
    fn does_not_flag_interface_with_two_implementations() {
        let src = "interface Store { get(k: string): string; }\nclass A implements Store { get(k: string) { return k; } }\nclass B implements Store { get(k: string) { return k; } }\n";
        let parsed = ParsedFile::parse(Language::TypeScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.ts"), &parsed);
        let diff = whole_file_diff("a.ts", src, Language::TypeScript);
        assert!(single_implementation_abstractions(&parsed, &diff, &symbols).is_empty());
    }

    #[test]
    fn flags_always_same_argument_as_unused_generality() {
        let src = "function render(node, useCache) { return node; }\nrender(a, true);\nrender(b, true);\nrender(c, true);\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        let f = unused_generality(&parsed, &diff, &symbols);
        assert_eq!(f.len(), 1, "got {f:?}");
        assert!(f[0].message.contains("useCache"));
    }

    #[test]
    fn stays_silent_on_ambiguous_names() {
        let src = "function run() { return inner(); }\nrun();\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        // A second declaration elsewhere makes the name ambiguous.
        symbols.add_file(
            std::path::Path::new("b.js"),
            Language::JavaScript,
            "function run() { return 1; }",
        );
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        assert!(pass_through_wrappers(&parsed, &diff, &symbols).is_empty());
    }
}
