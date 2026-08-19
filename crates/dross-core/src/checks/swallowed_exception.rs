//! Swallowed-exception / broad-catch detector (spec section 5b).
//!
//! Four AST-shape signals, no statistical baseline. This is the check that
//! validates the whole pipeline end-to-end.

use tree_sitter::Node;

use crate::ast::ParsedFile;
use crate::finding::{CheckId, Finding, Severity, SourceSpan};
use crate::lang::Language;

use super::{CheckContext, Check};

pub struct SwallowedExceptionCheck;

impl Check for SwallowedExceptionCheck {
    fn id(&self) -> CheckId {
        CheckId::SwallowedException
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for file in ctx.changed_files() {
            let Some(parsed) = ctx.parsed(&file.path) else {
                continue;
            };
            for handler in catch_handlers(parsed) {
                // Only report handlers the diff actually touched — Dross is a
                // pre-commit check, not a whole-repo linter.
                if !file.touches_range(handler.start_line, handler.end_line) {
                    continue;
                }
                findings.extend(evaluate(parsed, &handler, &file.path));
            }
        }
        findings
    }
}

struct CatchHandler<'a> {
    node: Node<'a>,
    body: Option<Node<'a>>,
    start_line: usize,
    end_line: usize,
    /// The caught type as written, e.g. `Exception`, `TypeError`, or `None`
    /// for a bare `except:` / parameterless `catch {}`.
    caught_type: Option<String>,
}

fn catch_handlers<'a>(file: &'a ParsedFile) -> Vec<CatchHandler<'a>> {
    let mut out = Vec::new();
    crate::ast::walk(file.root(), |node| {
        let is_handler = match file.language {
            Language::Python => node.kind() == "except_clause",
            _ => node.kind() == "catch_clause",
        };
        if !is_handler {
            return;
        }
        let (start_line, end_line) = file.line_span(node);
        out.push(CatchHandler {
            node,
            body: handler_body(file.language, node),
            start_line,
            end_line,
            caught_type: caught_type(file, node),
        });
    });
    out.sort_by_key(|h| h.start_line);
    out
}

fn handler_body<'a>(language: Language, node: Node<'a>) -> Option<Node<'a>> {
    match language {
        Language::Python => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .find(|c| c.kind() == "block")
        }
        _ => node.child_by_field_name("body"),
    }
}

fn caught_type(file: &ParsedFile, node: Node<'_>) -> Option<String> {
    match file.language {
        Language::Python => {
            // `except ValueError:` puts the type directly under the clause,
            // while `except ValueError as e:` wraps it in an `as_pattern`
            // whose first child is the type.
            let mut cursor = node.walk();
            let candidate = node
                .named_children(&mut cursor)
                .find(|c| !matches!(c.kind(), "block" | "comment"))?;
            let type_node = if candidate.kind() == "as_pattern" {
                let mut inner = candidate.walk();
                candidate.named_children(&mut inner).next()?
            } else {
                candidate
            };
            Some(file.text(type_node).trim().to_string())
        }
        _ => node
            .child_by_field_name("parameter")
            .and_then(|p| p.child_by_field_name("type").or(Some(p)))
            .map(|n| crate::ast::normalize_type(file.text(n))),
    }
}

fn evaluate(file: &ParsedFile, handler: &CatchHandler<'_>, path: &std::path::Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let span = SourceSpan {
        file: path.to_path_buf(),
        start_line: handler.start_line,
        end_line: handler.end_line,
    };

    let stats = body_stats(file, handler.body);

    // Signal 1: empty catch body.
    if stats.is_empty {
        findings.push(Finding::new(
            CheckId::SwallowedException,
            "empty-catch-body",
            Severity::Error,
            span.clone(),
            "Exception is caught and silently discarded",
            "The catch/except body is empty or contains only a no-op statement, \
             so the error is neither logged, rethrown, nor handled.",
        ));
        return findings;
    }

    // Signal 2: log-only catch.
    if stats.has_logging && !stats.surfaces_error() {
        findings.push(Finding::new(
            CheckId::SwallowedException,
            "log-only-catch",
            Severity::Warning,
            span.clone(),
            "Exception is logged but never surfaced to the caller",
            "The handler body contains a logging call but no rethrow, raise, or \
             error return, so the caller cannot tell the operation failed.",
        ));
    }

    // Signal 4: silent optimistic return.
    if let Some(literal) = stats.returns_literal.as_ref() {
        if !stats.surfaces_error() {
            findings.push(Finding::new(
                CheckId::SwallowedException,
                "silent-optimistic-return",
                Severity::Warning,
                span.clone(),
                "Failure path returns a default value instead of propagating the error",
                format!(
                    "The handler returns `{literal}` on the failure path with no \
                     caller-visible signal that the result is degraded."
                ),
            ));
        }
    }

    // Signal 3: overly broad catch type.
    if let Some(caught) = handler.caught_type.as_deref() {
        if is_broad_type(file.language, caught) {
            findings.push(Finding::new(
                CheckId::SwallowedException,
                "overly-broad-catch-type",
                Severity::Info,
                span.clone(),
                format!("Catches the broad type `{caught}`"),
                "Catching a broad base type also swallows unrelated failures \
                 (bugs, cancellation, out-of-memory) that the try block never \
                 intended to handle.",
            ));
        }
    } else if file.language == Language::Python && is_bare_except(file, handler.node) {
        findings.push(Finding::new(
            CheckId::SwallowedException,
            "overly-broad-catch-type",
            Severity::Info,
            span,
            "Bare `except:` catches every exception, including KeyboardInterrupt",
            "A bare except also catches SystemExit and KeyboardInterrupt, which \
             almost never should be handled here.",
        ));
    }

    findings
}

#[derive(Default)]
struct BodyStats {
    is_empty: bool,
    has_logging: bool,
    has_throw: bool,
    has_error_return: bool,
    returns_literal: Option<String>,
    statement_count: usize,
}

impl BodyStats {
    fn surfaces_error(&self) -> bool {
        self.has_throw || self.has_error_return
    }
}

fn body_stats(file: &ParsedFile, body: Option<Node<'_>>) -> BodyStats {
    let mut stats = BodyStats::default();
    let Some(body) = body else {
        stats.is_empty = true;
        return stats;
    };

    let mut cursor = body.walk();
    let meaningful: Vec<Node<'_>> = body
        .named_children(&mut cursor)
        .filter(|c| !matches!(c.kind(), "comment"))
        .collect();
    stats.statement_count = meaningful.len();

    if meaningful.is_empty() {
        stats.is_empty = true;
        return stats;
    }
    // `pass`, `;`, and `{}` are no-ops even though they parse as statements.
    if meaningful.iter().all(|n| is_noop_statement(file, *n)) {
        stats.is_empty = true;
        return stats;
    }

    crate::ast::walk(body, |node| {
        match node.kind() {
            "throw_statement" | "raise_statement" => stats.has_throw = true,
            "call_expression" | "call" => {
                let text = file.text(node);
                if looks_like_logging(text) {
                    stats.has_logging = true;
                }
                // `Promise.reject(...)` / `reject(...)` surfaces the error.
                if text.starts_with("reject(") || text.contains("Promise.reject") {
                    stats.has_error_return = true;
                }
            }
            "return_statement" => {
                if let Some(value) = returned_value(file, node) {
                    let text = value.trim().to_string();
                    if is_error_shaped(&text) {
                        stats.has_error_return = true;
                    } else if is_default_literal(&text) {
                        stats.returns_literal = Some(text);
                    }
                } else {
                    stats.returns_literal = Some("undefined".to_string());
                }
            }
            _ => {}
        }
    });

    stats
}

fn returned_value(file: &ParsedFile, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|c| c.kind() != "comment")
        .map(|n| file.text(n).to_string())
}

fn is_noop_statement(file: &ParsedFile, node: Node<'_>) -> bool {
    matches!(node.kind(), "pass_statement" | "empty_statement")
        || file.text(node).trim().is_empty()
}

fn looks_like_logging(text: &str) -> bool {
    const NEEDLES: [&str; 10] = [
        "console.", "logger.", "log.", "logging.", "print(", "warn(", "error(", "debug(",
        "info(", "trace(",
    ];
    let head = text.split('(').next().unwrap_or(text).to_ascii_lowercase();
    NEEDLES.iter().any(|n| {
        let n = n.trim_end_matches('(');
        head == n || head.ends_with(&format!(".{n}")) || head.starts_with(n)
    })
}

/// A returned value that explicitly communicates failure, e.g. `Err(...)`,
/// `{ ok: false }`, or a rejected promise.
fn is_error_shaped(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("err(")
        || lower.contains("promise.reject")
        || lower.contains("ok: false")
        || lower.contains("ok:false")
        || lower.contains("success: false")
        || lower.contains("success:false")
        || (lower.contains("error") && !lower.starts_with("null"))
}

/// A default/empty value that hides the failure from the caller.
fn is_default_literal(text: &str) -> bool {
    matches!(
        text.trim(),
        "null" | "None" | "undefined" | "0" | "-1" | "\"\"" | "''" | "[]" | "{}" | "false" | "False"
    )
}

fn is_broad_type(language: Language, caught: &str) -> bool {
    let t = caught.trim();
    match language {
        Language::Python => matches!(t, "Exception" | "BaseException" | "(Exception)" | "(BaseException)"),
        _ => matches!(t, "Error" | "any" | "unknown" | "Exception"),
    }
}

fn is_bare_except(file: &ParsedFile, node: Node<'_>) -> bool {
    let text = file.text(node);
    let head = text.lines().next().unwrap_or("").trim();
    head == "except:" || head.starts_with("except:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ParsedFile;

    fn run_on(language: Language, src: &str) -> Vec<Finding> {
        let file = ParsedFile::parse(language, src).unwrap();
        let path = std::path::PathBuf::from("t");
        catch_handlers(&file)
            .iter()
            .flat_map(|h| evaluate(&file, h, &path))
            .collect()
    }

    fn signals(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.signal.as_str()).collect()
    }

    #[test]
    fn flags_empty_js_catch() {
        let f = run_on(Language::JavaScript, "try { g(); } catch (e) {}");
        assert_eq!(signals(&f), vec!["empty-catch-body"]);
    }

    #[test]
    fn flags_python_except_pass() {
        let f = run_on(Language::Python, "try:\n    g()\nexcept ValueError:\n    pass\n");
        assert_eq!(signals(&f), vec!["empty-catch-body"]);
    }

    #[test]
    fn flags_log_only_catch() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) { console.error(e); }",
        );
        assert!(signals(&f).contains(&"log-only-catch"));
    }

    #[test]
    fn does_not_flag_log_and_rethrow() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) { console.error(e); throw e; }",
        );
        assert!(!signals(&f).contains(&"log-only-catch"));
    }

    #[test]
    fn flags_silent_optimistic_return() {
        let f = run_on(
            Language::JavaScript,
            "try { return parse(x); } catch (e) { return null; }",
        );
        assert!(signals(&f).contains(&"silent-optimistic-return"));
    }

    #[test]
    fn flags_broad_python_exception() {
        let f = run_on(
            Language::Python,
            "try:\n    g()\nexcept Exception as e:\n    logging.error(e)\n",
        );
        assert!(signals(&f).contains(&"overly-broad-catch-type"));
    }

    #[test]
    fn does_not_flag_narrow_rethrowing_handler() {
        let f = run_on(
            Language::Python,
            "try:\n    g()\nexcept ValueError as e:\n    raise RuntimeError(e)\n",
        );
        assert!(f.is_empty(), "unexpected findings: {:?}", signals(&f));
    }
}
