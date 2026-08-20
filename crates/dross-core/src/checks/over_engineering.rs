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
            // Test code is exempt from the indirection and generality signals.
            // A test that performs a single assertion looks exactly like a
            // pass-through wrapper, and a fixture helper called from one test
            // looks exactly like unused generality — both are normal there.
            // Found by running Dross against its own repository, where
            // `test_roundtrip` was flagged for forwarding to `assertEqual`.
            if super::tautological_test::is_test_path(&file.path) {
                continue;
            }
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
        // The body being a single call is not enough. `findSummary(ref)`
        // calling `findTag(ref, "@summary")` binds an argument, and
        // `fixAllItemsIds` calling `.forEach(cb)` hides the logic in the
        // callback. Both were reported as pass-throughs in the benchmark.
        // A real wrapper forwards its own parameters, unchanged and in order.
        if !forwards_parameters_verbatim(parsed, node, &func.params) {
            continue;
        }
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

/// True when the single call forwards exactly the function's own parameters,
/// in order, adding nothing.
fn forwards_parameters_verbatim(
    file: &ParsedFile,
    func: Node<'_>,
    params: &[crate::ast::Param],
) -> bool {
    let Some(call) = single_call(func) else {
        return false;
    };
    let Some(args) = call.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    let actual: Vec<String> = args
        .named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .map(|c| {
            let kind = c.kind();
            // A function-literal argument carries logic of its own, so the
            // wrapper is not a pure forward.
            if kind.contains("function") || kind.contains("arrow") || kind == "lambda" {
                String::from("<callback>")
            } else {
                file.text(c).trim().to_string()
            }
        })
        .collect();

    actual.len() == params.len()
        && actual
            .iter()
            .zip(params)
            .all(|(arg, param)| arg == &param.name)
}

/// The single call expression a body consists of, if that is all it contains.
fn single_call<'a>(func: Node<'a>) -> Option<Node<'a>> {
    let body = func.child_by_field_name("body")?;
    let mut cursor = body.walk();
    let statements: Vec<Node<'a>> = body
        .named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect();
    if statements.len() != 1 {
        return None;
    }
    let stmt = statements[0];
    let mut c = stmt.walk();
    let expr = match stmt.kind() {
        "return_statement" | "expression_statement" => stmt
            .named_children(&mut c)
            .find(|n| n.kind() != "comment")?,
        _ => return None,
    };
    let call = if expr.kind() == "await_expression" {
        let mut c2 = expr.walk();
        expr.named_children(&mut c2).next()?
    } else {
        expr
    };
    matches!(call.kind(), "call_expression" | "call").then_some(call)
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
        // A type-level contract — an event map, a props or payload shape — is
        // implemented once by design and is not a polymorphic abstraction.
        // socket.io's `SocketReservedEventsMap` and its siblings were all
        // false positives. Requiring a declared method keeps this on
        // behavioural interfaces.
        if !declares_a_method(node) {
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

/// True when a type declares behaviour rather than only data members.
fn declares_a_method(node: Node<'_>) -> bool {
    let mut found = false;
    walk(node, |n| {
        if matches!(
            n.kind(),
            "method_signature" | "method_definition" | "function_signature" | "call_signature"
        ) {
            found = true;
        }
    });
    found
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

/// Matches factory-ish names on word boundaries.
///
/// Substring matching flagged `test_idmaker_...` (through "make") and
/// `_apply_long_str_strategy`, neither of which is a factory.
fn is_factory_name(name: &str) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .flat_map(split_camel)
        .any(|w| {
            matches!(
                w.to_ascii_lowercase().as_str(),
                "factory" | "create" | "make" | "build"
            )
        })
}

/// Splits `createCookieJar` into `create`, `Cookie`, `Jar`.
fn split_camel(part: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = part.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i].is_ascii_uppercase() {
            out.push(&part[start..i]);
            start = i;
        }
    }
    if start < part.len() {
        out.push(&part[start..]);
    }
    out
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
    // A factory that unconditionally constructs one object is normal code, not
    // overkill scaffolding. Only real dispatch counts, so `create_app` and
    // similar application factories no longer register as one-variant
    // registries — every such finding in the benchmark was a false positive.
    let _ = file;
    cases.max(object_entries)
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
            // Only a literal means anything here. The benchmark reported
            // "parameter `exc_info` is always `exc_info`" — the argument was a
            // variable sharing the parameter's name, or a Python keyword
            // fragment that positional matching had mis-indexed. All twelve
            // sampled findings were false positives.
            if !is_literal_argument(&distinct[0]) {
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

/// True for arguments that are constants rather than expressions.
///
/// "always passed `true`" is a finding; "always passed `options`" is not — the
/// latter says nothing about whether the parameter is ever exercised.
fn is_literal_argument(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.contains('=') || t.contains('(') {
        return false;
    }
    matches!(
        t,
        "true" | "false" | "null" | "undefined" | "None" | "True" | "False"
    ) || t.parse::<f64>().is_ok()
        || t.starts_with('"')
        || t.starts_with('\'')
        || t.starts_with('`')
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

        // Complexity the change adds, not the complexity of everything it
        // touched — otherwise a reformat accumulates the whole file.
        let old_parsed = ctx.parsed_old(file);
        total_complexity +=
            metrics::added_complexity(parsed, old_parsed.as_ref(), |s, e| file.touches_range(s, e));

        if anchor.is_none()
            && let Some(func) = parsed
                .functions()
                .into_iter()
                .find(|f| file.touches_range(f.start_line, f.end_line))
        {
            anchor = Some(SourceSpan {
                file: file.path.clone(),
                start_line: func.start_line,
                end_line: func.end_line,
            });
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

    /// Regression: `findSummary(ref)` calling `findTag(ref, "@summary")` binds
    /// an argument, and a body that is one `.forEach(cb)` hides its logic in
    /// the callback. Both were reported as pass-throughs across the benchmark.
    #[test]
    fn a_wrapper_that_binds_an_argument_is_not_a_pass_through() {
        let src =
            "function findSummary(ref) { return findTag(ref, '@summary'); }\nfindSummary(x);\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        assert!(pass_through_wrappers(&parsed, &diff, &symbols).is_empty());
    }

    #[test]
    fn a_body_that_is_one_call_with_a_callback_is_not_a_pass_through() {
        let src = "function fixAll(data) { return each(data, (v) => { if (v) { fix(v); } }); }\nfixAll(d);\n";
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let mut symbols = SymbolTable::new();
        symbols.add_parsed(std::path::Path::new("a.js"), &parsed);
        let diff = whole_file_diff("a.js", src, Language::JavaScript);
        assert!(pass_through_wrappers(&parsed, &diff, &symbols).is_empty());
    }

    /// Regression: the name matcher hit substrings inside unrelated words, so
    /// `test_idmaker_...` matched "make" and `_apply_long_str_strategy`
    /// matched "strategy".
    #[test]
    fn factory_names_match_on_word_boundaries() {
        assert!(is_factory_name("createExporter"));
        assert!(is_factory_name("create_app"));
        assert!(is_factory_name("build_thing"));
        assert!(!is_factory_name("test_idmaker_long_string"));
        // A *_strategy function is a strategy implementation, not a registry;
        // both of these were false positives in the benchmark.
        assert!(!is_factory_name("_apply_long_str_strategy"));
        assert!(!is_factory_name("singleFetchLoaderFetcherStrategy"));
    }

    /// Regression: "always passed `exc_info`" reported a variable name, not a
    /// constant, so the parameter was in fact exercised.
    #[test]
    fn unused_generality_requires_a_literal() {
        assert!(is_literal_argument("true"));
        assert!(is_literal_argument("0"));
        assert!(is_literal_argument("\"csv\""));
        assert!(!is_literal_argument("exc_info"));
        assert!(!is_literal_argument("by_alias=self.by_alias"));
        assert!(!is_literal_argument("compute()"));
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
