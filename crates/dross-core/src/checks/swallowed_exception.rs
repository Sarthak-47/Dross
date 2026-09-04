//! Swallowed-exception / broad-catch detector (spec section 5b).
//!
//! Four AST-shape signals, no statistical baseline. This is the check that
//! validates the whole pipeline end-to-end.

use tree_sitter::Node;

use crate::ast::ParsedFile;
use crate::finding::{CheckId, Finding, Severity, SourceSpan};
use crate::lang::Language;

use super::{Check, CheckContext};

pub struct SwallowedExceptionCheck;

impl Check for SwallowedExceptionCheck {
    fn id(&self) -> CheckId {
        CheckId::SwallowedException
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for file in ctx.changed_files() {
            // Test code swallows deliberately: awaiting a known rejection,
            // asserting on a warning, probing for optional dependencies. Every
            // sampled finding in a test file was a false positive.
            if super::tautological_test::is_non_production_path(&file.path) {
                continue;
            }
            let Some(parsed) = ctx.parsed(&file.path) else {
                continue;
            };
            for handler in catch_handlers(parsed) {
                // Only report handlers the diff actually touched — Dross is a
                // pre-commit check, not a whole-repo linter.
                if !file.touches_range(handler.start_line, handler.end_line) {
                    continue;
                }
                // Rust keeps its tests inside the file they test. The
                // path-based rule above cannot see them.
                if crate::ast::is_inside_test_module(parsed, handler.node) {
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
            // Rust has no catch. The equivalent is the arm of a `match` that
            // handles the error case, which is where a `Result` is either dealt
            // with or quietly dropped.
            Language::Rust => is_error_arm(file, node),
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

/// Whether a node is the `Err(..)` arm of a `match`.
///
/// Only `Err`, and only in a match: `if let Ok(v) = ..` without an else is the
/// other shape that drops an error, but it is idiomatic enough — and used
/// deliberately often enough — that reporting it would repeat the false
/// positives this check spent four rounds removing.
fn is_error_arm(file: &ParsedFile, node: Node<'_>) -> bool {
    if node.kind() != "match_arm" {
        return false;
    }
    let Some(pattern) = node.child_by_field_name("pattern") else {
        return false;
    };
    file.text(pattern).trim_start().starts_with("Err")
}

fn handler_body<'a>(language: Language, node: Node<'a>) -> Option<Node<'a>> {
    match language {
        Language::Python => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .find(|c| c.kind() == "block")
        }
        // A match arm's body is its `value`: either a block, or a single
        // expression as in `Err(e) => return Err(e)`.
        Language::Rust => node.child_by_field_name("value"),
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
        // `catch (e)` declares a binding, not a type. Falling back to the
        // binding made every JavaScript handler look narrowly typed.
        _ => node
            .child_by_field_name("parameter")
            .and_then(|p| p.child_by_field_name("type"))
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
    //
    // A *narrow* catch that ignores is targeted: scrapy's
    // `except CannotListenError: pass` while probing ports, pydantic's
    // `except TypeError: pass`. The author named the one thing they expected.
    // A broad or untyped catch names nothing, and that is what this is for.
    let narrow_catch = handler
        .caught_type
        .as_deref()
        .is_some_and(|c| !is_broad_type(file.language, c));

    if stats.is_empty && !stats.is_documented && !narrow_catch {
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
    if stats.is_empty {
        // Documented and empty: the decision is recorded, so nothing to say.
        return findings;
    }

    // Signal 2: log-only catch.
    //
    // A *narrow* catch that returns explicitly is stating a contract: pytest's
    // `_read_pyc` catches OSError, traces it, and returns None, which every
    // caller checks. Reporting that is second-guessing a deliberate design.
    // A broad catch has made no such statement.
    let narrow_with_explicit_return = stats.returns_literal.is_some()
        && handler
            .caught_type
            .as_deref()
            .is_some_and(|c| !is_broad_type(file.language, c));

    // A log line that states the failure is expected is the author writing
    // the decision down: "ignore malformed buffer", "websocket closed before
    // onclose event". That is the same reasoning the empty-catch signal
    // applies to a comment, and these were the bulk of what remained.
    let states_intent = stats.log_text.as_deref().is_some_and(declares_expected);

    // A comment explaining the decision counts wherever the author put it. The
    // empty-catch signal already honours one inside the handler; socket.io
    // puts it above the `try` instead —
    //
    //     // Sometimes the websocket has already been closed but the browser
    //     // didn't have a chance of informing us about it yet, in that case
    //     // send will throw an error
    //     try { this.doWrite(packet, data); } catch (e) { debug(...); }
    //
    // which is the same decision, recorded in the more natural place.
    let documented = stats.is_documented || try_is_documented(file, handler.node);

    if stats.has_logging
        && !stats.surfaces_error()
        && !narrow_with_explicit_return
        && !states_intent
        && !documented
    {
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
    //
    // Returning a default on failure is the documented contract far more often
    // than it is a concealment, and two shapes say so outright.
    let declared_safe = enclosing_function_name(file, handler.node)
        .is_some_and(|name| name_declares_a_safe_result(&name));
    let matches_contract = stats
        .returns_literal
        .as_ref()
        .is_some_and(|literal| returns_same_outside(file, handler, literal));

    if let Some(literal) = stats.returns_literal.as_ref()
        && !stats.surfaces_error()
        && !declared_safe
        && !matches_contract
    {
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

    // Signal 3: overly broad catch type.
    //
    // Breadth alone is a style opinion, not a defect. Every sampled finding was
    // Flask's top-level request handler, which must catch everything and then
    // delegates to `handle_user_exception`. The signal only means something
    // when the broad catch also fails to surface what it caught.
    let breadth_matters = !stats.surfaces_error();
    if let Some(caught) = handler.caught_type.as_deref() {
        if breadth_matters && is_broad_type(file.language, caught) {
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
    } else if breadth_matters
        && file.language == Language::Python
        && is_bare_except(file, handler.node)
    {
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
    /// The handler carries a comment explaining the decision.
    is_documented: bool,
    has_logging: bool,
    /// Text of the logging call, used to spot a message that documents intent.
    log_text: Option<String>,
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

/// The name of the function a handler sits in, if it has one.
fn enclosing_function_name(file: &ParsedFile, node: Node<'_>) -> Option<String> {
    let mut current = node.parent();
    while let Some(n) = current {
        if crate::ast::is_function_node(file.language, n.kind())
            && let Some(name) = n.child_by_field_name("name")
        {
            return Some(file.text(name).trim().to_string());
        }
        current = n.parent();
    }
    None
}

/// Whether a function's name states that a default result is its contract.
///
/// axios wraps `String(value)` in `stringifySafely` and returns `''` when it
/// throws. The name is the documentation, and it is the same reasoning the
/// empty-catch signal applies to a comment.
fn name_declares_a_safe_result(name: &str) -> bool {
    let lower = name.trim_start_matches('_').to_ascii_lowercase();
    lower.ends_with("safely")
        || lower.ends_with("_safe")
        || lower.ends_with("ornone")
        || lower.ends_with("_or_none")
        || lower.ends_with("ordefault")
        || lower.ends_with("_or_default")
        || lower.starts_with("safe_")
        || lower.starts_with("try_")
        // A following character is required, so a function simply named
        // `maybe` does not qualify — the prefix has to be qualifying something.
        || (lower.starts_with("maybe") && lower.len() > "maybe".len())
}

/// Whether the enclosing function already returns this same value somewhere
/// outside the handler.
///
/// pytest's `_read_pyc` returns `None` on five separate validation failures
/// before the `except` that also returns `None`. Returning it once more from a
/// handler is that function's established contract, not a concealed failure —
/// every caller already has to handle it.
fn returns_same_outside(file: &ParsedFile, handler: &CatchHandler<'_>, literal: &str) -> bool {
    let Some(func) = enclosing_function_node(file, handler.node) else {
        return false;
    };
    let (h_start, h_end) = (handler.node.start_byte(), handler.node.end_byte());
    let mut found = false;
    crate::ast::walk(func, |n| {
        if found || n.kind() != "return_statement" {
            return;
        }
        // Skip returns inside the handler itself.
        if n.start_byte() >= h_start && n.end_byte() <= h_end {
            return;
        }
        let text = file.text(n);
        let value = text
            .trim()
            .trim_start_matches("return")
            .trim()
            .trim_end_matches(';')
            .trim();
        if value == literal || (value.is_empty() && literal == "undefined") {
            found = true;
        }
    });
    found
}

fn enclosing_function_node<'a>(file: &ParsedFile, node: Node<'a>) -> Option<Node<'a>> {
    let mut current = node.parent();
    while let Some(n) = current {
        if crate::ast::is_function_node(file.language, n.kind()) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Whether a comment immediately precedes the `try` this handler belongs to.
///
/// Only the statement directly above counts. A comment further up is about
/// something else, and treating any nearby comment as documentation would
/// silence the signal wherever code is commented at all.
fn try_is_documented(file: &ParsedFile, handler: Node<'_>) -> bool {
    let Some(try_node) = handler.parent() else {
        return false;
    };
    let Some(previous) = try_node.prev_named_sibling() else {
        return false;
    };
    if previous.kind() != "comment" {
        return false;
    }
    // Adjacent, not merely earlier: at most one blank line between them.
    let gap = try_node
        .start_position()
        .row
        .saturating_sub(previous.end_position().row);
    gap <= 1 && !file.text(previous).trim().is_empty()
}

fn body_stats(file: &ParsedFile, body: Option<Node<'_>>) -> BodyStats {
    let mut stats = BodyStats::default();
    let Some(body) = body else {
        stats.is_empty = true;
        return stats;
    };

    let mut cursor = body.walk();
    let children: Vec<Node<'_>> = body.named_children(&mut cursor).collect();
    // An empty catch carrying an explanation is a decision someone made and
    // wrote down — "Ignore failures from custom stack hooks", "no-op, use
    // default empty object", "node-fetch throws when the request is closed
    // abnormally". Those were the false positives; the ones worth reporting
    // had nothing at all.
    stats.is_documented = children.iter().any(|c| c.kind() == "comment");
    let meaningful: Vec<Node<'_>> = children
        .into_iter()
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

    // A match arm can answer with an expression rather than a block:
    // `Err(e) => Err(e)` hands the failure straight back, and there is no
    // return statement to notice.
    if file.language == Language::Rust
        && body.kind() != "block"
        && is_error_shaped(file.text(body).trim())
    {
        stats.has_error_return = true;
    }

    crate::ast::walk(body, |node| {
        match node.kind() {
            "throw_statement" | "raise_statement" => stats.has_throw = true,
            // Rust: `panic!`, `unreachable!`, `todo!` and `?` all stop the
            // handler from being where the failure ends.
            "macro_invocation" => {
                let text = file.text(node);
                let name = text.split('!').next().unwrap_or("").trim();
                if matches!(name, "panic" | "unreachable" | "todo" | "unimplemented") {
                    stats.has_throw = true;
                } else if rust_macro_logs(name) {
                    stats.has_logging = true;
                    if stats.log_text.is_none() {
                        stats.log_text = Some(text.to_string());
                    }
                }
            }
            "try_expression" => stats.has_throw = true,
            "call_expression" | "call" => {
                let text = file.text(node);
                if looks_like_logging(text) {
                    stats.has_logging = true;
                    if stats.log_text.is_none() {
                        stats.log_text = Some(text.to_string());
                    }
                }
                // `Promise.reject(...)` / `reject(...)` surfaces the error.
                if text.starts_with("reject(") || text.contains("Promise.reject") {
                    stats.has_error_return = true;
                }
                // Throwing and returning are not the only ways to surface a
                // failure. The benchmark found handlers that emit an error
                // event, hand the error to a callback, or pass it to a
                // reporter — socket.io's `emit("connection_error", ...)`,
                // got's `_beforeError`, black's `report.failed(path, exc)`.
                // All were reported as swallowed, and all were wrong.
                if surfaces_error_via_call(text) {
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

/// True when a call hands the failure somewhere the caller can observe.
///
/// Deliberately narrow: the call has to name an error, or be a conventional
/// error-first callback. A generic `cleanup()` does not count.
fn surfaces_error_via_call(text: &str) -> bool {
    // `console.error` and `logger.error` end in "error" but only write to a
    // log. Logging is precisely what this check is about, so it never counts
    // as surfacing.
    // `warnings.warn(...)` is a channel rather than a log line: it is routed
    // through the warnings filter, which callers can escalate to an error, so
    // the failure remains visible to them. Checked before the logging guard
    // below, which would otherwise match it on `warn(`.
    if text.trim_start().starts_with("warnings.warn") {
        return true;
    }

    // A call that records the traceback is not a log line either. `logging
    // .exception(...)`, and any logging call passing `exc_info`, preserve what
    // was raised and where it came from, which is the difference between
    // reporting a failure and mentioning it.
    //
    // This is ruff's position in BLE001, which exempts handlers that re-raise
    // or log with exc_info. Comparing Dross against it on the corpus, these
    // were every one of the nine handlers the two tools disagreed about —
    // scrapy's `logger.error(..., exc_info=True)` and `logger.exception(...)`
    // among them. Agreeing with the more widely used rule is the right call.
    let call_head = text.split('(').next().unwrap_or(text);
    if call_head.trim_end().ends_with(".exception") || text.contains("exc_info") {
        return true;
    }
    if looks_like_logging(text) {
        return false;
    }
    let head = text.split('(').next().unwrap_or(text).to_ascii_lowercase();
    let leaf = head.rsplit('.').next().unwrap_or(&head).to_string();

    // report.failed(...), this.emit("connection_error"), _beforeError(...)
    // Delegating to an exception handler, e.g. `self.handle_user_exception(e)`.
    if leaf.starts_with("handle") {
        let lower = text.to_ascii_lowercase();
        if lower.contains("exc") || lower.contains("error") {
            return true;
        }
    }

    if leaf.ends_with("error")
        || leaf.contains("beforeerror")
        || leaf.contains("onerror")
        || leaf.contains("handleerror")
        || leaf.contains("reporterror")
        || matches!(leaf.as_str(), "failed" | "fail" | "abort" | "errback")
    {
        return true;
    }

    // An emit/dispatch/publish whose event name mentions an error.
    if leaf.starts_with("emit")
        || leaf.starts_with("dispatch")
        || leaf.starts_with("publish")
        || leaf.starts_with("trigger")
        || leaf.starts_with("send")
        || leaf.starts_with("notify")
    {
        let lower = text.to_ascii_lowercase();
        return lower.contains("error") || lower.contains("fail");
    }

    // Node-style error-first callback. The argument is usually the caught
    // binding — `callback(e)` — so requiring the word "error" missed it.
    // A callback invoked with any argument on the failure path is passing the
    // failure on; a bare `next()` is not.
    if matches!(
        leaf.as_str(),
        "callback" | "cb" | "next" | "done" | "errback"
    ) || leaf.ends_with("witherror")
    {
        let args = text
            .split_once('(')
            .map(|(_, rest)| rest.trim_end_matches(')').trim())
            .unwrap_or("");
        return !args.is_empty();
    }

    false
}

fn returned_value(file: &ParsedFile, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|c| c.kind() != "comment")
        .map(|n| file.text(n).to_string())
}

fn is_noop_statement(file: &ParsedFile, node: Node<'_>) -> bool {
    matches!(node.kind(), "pass_statement" | "empty_statement") || file.text(node).trim().is_empty()
}

/// True when a message says the failure was expected.
///
/// Deliberately a small, literal list. It is looking for an author stating
/// intent, not inferring it.
fn declares_expected(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "ignore",
        "ignoring",
        "best effort",
        "best-effort",
        "expected",
        "harmless",
        "not fatal",
        "non-fatal",
        "optional",
        "unsupported",
        "no-op",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Rust logs through macros, so the call-shaped heuristic never saw them.
///
/// Covers the `log` and `tracing` families and the two `eprintln`-shaped
/// escapes. `println!` is deliberately absent: writing to stdout in a library
/// is not error reporting.
fn rust_macro_logs(name: &str) -> bool {
    let leaf = name.rsplit("::").next().unwrap_or(name);
    matches!(
        leaf,
        "error" | "warn" | "info" | "debug" | "trace" | "eprintln" | "eprint"
    )
}

fn looks_like_logging(text: &str) -> bool {
    const NEEDLES: [&str; 10] = [
        "console.", "logger.", "log.", "logging.", "print(", "warn(", "error(", "debug(", "info(",
        "trace(",
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
    // Rust: returning `Err(..)` hands the failure back to the caller.
    if text.trim_start().starts_with("Err(") {
        return true;
    }
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
///
/// Booleans are deliberately excluded. `return false` from a handler is the
/// function's answer, not a value shaped like success — there is nothing for it
/// to be mistaken for. Every finding of this kind in the benchmark was a
/// predicate saying "no": `hasDependency`, `shouldBypassProxy`, zod's IP
/// validators, socket.io's payload check. Inspecting the enclosing function
/// missed `return Boolean(...)` forms; the returned value itself is reliable.
fn is_default_literal(text: &str) -> bool {
    matches!(
        text.trim(),
        "null" | "None" | "undefined" | "0" | "-1" | "\"\"" | "''" | "[]" | "{}"
    )
}

fn is_broad_type(language: Language, caught: &str) -> bool {
    let t = caught.trim();
    match language {
        Language::Python => matches!(
            t,
            "Exception" | "BaseException" | "(Exception)" | "(BaseException)"
        ),
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

    /// Regression: an empty catch with a comment is a recorded decision.
    /// Axios, react-router and socket.io all had one, and all were false
    /// positives.
    #[test]
    fn a_documented_empty_catch_is_not_reported() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) {
  // Ignore failures from custom stack hooks.
}",
        );
        assert!(f.is_empty(), "got {:?}", signals(&f));
    }

    #[test]
    fn an_undocumented_empty_catch_is_still_reported() {
        let f = run_on(Language::JavaScript, "try { g(); } catch (e) {}");
        assert!(signals(&f).contains(&"empty-catch-body"));
    }

    /// A narrow `except X: pass` names the one thing the author expected and
    /// ignores it deliberately. A broad or untyped handler names nothing.
    #[test]
    fn a_narrow_empty_except_is_deliberate_but_a_broad_one_is_not() {
        let narrow = run_on(
            Language::Python,
            "try:
    g()
except ValueError:
    pass
",
        );
        assert!(
            !signals(&narrow).contains(&"empty-catch-body"),
            "got {:?}",
            signals(&narrow)
        );

        let broad = run_on(
            Language::Python,
            "try:
    g()
except Exception:
    pass
",
        );
        assert!(signals(&broad).contains(&"empty-catch-body"));

        let bare = run_on(
            Language::Python,
            "try:
    g()
except:
    pass
",
        );
        assert!(signals(&bare).contains(&"empty-catch-body"));
    }

    #[test]
    fn flags_log_only_catch() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) { console.error(e); }",
        );
        assert!(signals(&f).contains(&"log-only-catch"));
    }

    /// Regression: pytest's `_read_pyc` catches OSError, traces it, and returns
    /// None as its documented contract. A narrow catch that returns explicitly
    /// has stated its behaviour.
    #[test]
    fn a_narrow_catch_that_returns_is_a_contract_not_a_swallow() {
        let f = run_on(
            Language::Python,
            "def read(p):
    try:
        return load(p)
    except OSError as e:
        trace(e)
        return None
",
        );
        assert!(
            !signals(&f).contains(&"log-only-catch"),
            "got {:?}",
            signals(&f)
        );
    }

    #[test]
    fn a_broad_catch_that_logs_and_returns_is_still_reported() {
        let f = run_on(
            Language::Python,
            "def read(p):
    try:
        return load(p)
    except Exception as e:
        logging.error(e)
        return None
",
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

    /// Regression: a handler that hands the error to an emitter, a callback,
    /// or a reporter is not swallowing it. socket.io, got, and black were all
    /// reported as log-only across the benchmark.
    #[test]
    fn error_surfaced_through_a_call_is_not_swallowed() {
        for src in [
            "try { g(); } catch (e) { debug('x'); this.emit('connection_error', { error: e }); }",
            "try { g(); } catch (e) { console.error(e); callback(e); }",
        ] {
            let f = run_on(Language::JavaScript, src);
            assert!(
                !signals(&f).contains(&"log-only-catch"),
                "should not be log-only: {src}"
            );
        }

        let py = run_on(
            Language::Python,
            "try:\n    g()\nexcept Exception as exc:\n    traceback.print_exc()\n    report.failed(path, exc)\n",
        );
        assert!(!signals(&py).contains(&"log-only-catch"));
    }

    /// Regression: breadth alone is a style opinion. Every sampled finding was
    /// Flask's top-level request handler, which must catch everything and
    /// delegates to handle_user_exception.
    #[test]
    fn a_broad_catch_that_delegates_is_not_reported() {
        let f = run_on(
            Language::Python,
            "try:\n    rv = self.dispatch_request()\nexcept Exception as e:\n    rv = self.handle_user_exception(e)\n",
        );
        assert!(
            !signals(&f).contains(&"overly-broad-catch-type"),
            "got {:?}",
            signals(&f)
        );
    }

    #[test]
    fn a_broad_catch_that_swallows_is_still_reported() {
        let f = run_on(
            Language::Python,
            "try:\n    g()\nexcept Exception as e:\n    logging.error(e)\n",
        );
        assert!(signals(&f).contains(&"overly-broad-catch-type"));
    }

    /// Regression: a handler returning a boolean is answering, not hiding a
    /// failure — including `return Boolean(...)` forms that inspecting the
    /// enclosing function did not catch.
    #[test]
    fn a_predicate_returning_false_on_failure_is_not_a_silent_default() {
        let f = run_on(
            Language::JavaScript,
            "function isIPv6(value) { try { new URL(`http://[${value}]`); return true; } catch { return false; } }",
        );
        assert!(
            !signals(&f).contains(&"silent-optimistic-return"),
            "got {:?}",
            signals(&f)
        );
    }

    #[test]
    fn a_value_returning_function_still_reports_a_silent_default() {
        let f = run_on(
            Language::JavaScript,
            "function parsePort(raw) { try { return Number.parseInt(raw, 10); } catch (e) { return 0; } }",
        );
        assert!(signals(&f).contains(&"silent-optimistic-return"));
    }

    #[test]
    fn does_not_flag_narrow_rethrowing_handler() {
        let f = run_on(
            Language::Python,
            "try:\n    g()\nexcept ValueError as e:\n    raise RuntimeError(e)\n",
        );
        assert!(f.is_empty(), "unexpected findings: {:?}", signals(&f));
    }

    /// Regression: socket.io logged "ignore malformed buffer" and returned.
    /// The message is the author recording the decision, exactly as a comment
    /// records it for an empty handler.
    #[test]
    fn a_log_message_stating_intent_is_not_a_swallow() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) { debug('ignore malformed buffer'); }",
        );
        assert!(
            !signals(&f).contains(&"log-only-catch"),
            "{:?}",
            signals(&f)
        );
    }

    /// Regression: socket.io's "websocket closed before onclose event" is a
    /// race the author knew about.
    #[test]
    fn a_log_message_calling_the_failure_expected_is_not_a_swallow() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) { debug('closed before onclose, expected'); }",
        );
        assert!(!signals(&f).contains(&"log-only-catch"));
    }

    /// The intent list must not swallow the signal itself: an ordinary error
    /// log is still a log-only catch.
    #[test]
    fn an_ordinary_error_log_is_still_reported() {
        let f = run_on(
            Language::JavaScript,
            "try { g(); } catch (e) { console.log('failed to load config'); }",
        );
        assert!(signals(&f).contains(&"log-only-catch"), "{:?}", signals(&f));
    }

    /// Regression: requests warned about a dependency version mismatch.
    /// `warnings.warn` is a channel callers can escalate to an error, not a
    /// log line that disappears.
    #[test]
    fn a_python_warning_surfaces_the_failure() {
        let f = run_on(
            Language::Python,
            "try:
    g()
except Exception as e:
    warnings.warn(str(e))
",
        );
        assert!(
            !signals(&f).contains(&"log-only-catch"),
            "{:?}",
            signals(&f)
        );
    }

    /// axios wraps `String(value)` and returns `''` when it throws. The name
    /// is the documentation.
    #[test]
    fn a_name_that_promises_a_safe_result_is_not_concealing_one() {
        let f = run_on(
            Language::JavaScript,
            "function stringifySafely(value) {
  try {
    return String(value);
  } catch (err) {
    return '';
  }
}",
        );
        assert!(
            !signals(&f).contains(&"silent-optimistic-return"),
            "{:?}",
            signals(&f)
        );

        // The same body under a name that promises nothing still reports.
        let g = run_on(
            Language::JavaScript,
            "function render(value) {
  try {
    return String(value);
  } catch (err) {
    return '';
  }
}",
        );
        assert!(
            signals(&g).contains(&"silent-optimistic-return"),
            "{:?}",
            signals(&g)
        );
    }

    /// pytest's `_read_pyc` returns None on five validation failures before the
    /// `except` that also returns None. Every caller already handles it.
    #[test]
    fn returning_what_the_function_already_returns_is_its_contract() {
        let f = run_on(
            Language::Python,
            "def _read_pyc(source, pyc):
    if not pyc.exists():
        return None
    if bad_magic(pyc):
        return None
    try:
        return marshal.load(pyc)
    except Exception:
        return None
",
        );
        assert!(
            !signals(&f).contains(&"silent-optimistic-return"),
            "{:?}",
            signals(&f)
        );
    }

    /// The guard must not swallow the signal: a handler returning a default
    /// the function never otherwise returns is still reported.
    #[test]
    fn a_default_the_function_never_otherwise_returns_is_still_reported() {
        let f = run_on(
            Language::Python,
            "def load_config(path):
    data = read(path)
    try:
        return parse(data)
    except Exception:
        return None
",
        );
        assert!(
            signals(&f).contains(&"silent-optimistic-return"),
            "{:?}",
            signals(&f)
        );
    }

    #[test]
    fn safe_result_names_are_matched_on_shape_not_substring() {
        for name in [
            "stringifySafely",
            "safe_join",
            "try_parse",
            "get_or_none",
            "maybeParse",
        ] {
            assert!(name_declares_a_safe_result(name), "{name}");
        }
        for name in ["render", "safety_check", "retry", "maybe", "parse"] {
            assert!(!name_declares_a_safe_result(name), "{name}");
        }
    }

    /// socket.io explains this one above the `try` rather than inside the
    /// handler, which is the more natural place to put it.
    #[test]
    fn a_comment_above_the_try_documents_the_handler_too() {
        let f = run_on(
            Language::JavaScript,
            "function send(packet, data) {
  // Sometimes the websocket has already been closed but the browser
  // didn't have a chance of informing us yet, in which case send throws.
  try {
    this.doWrite(packet, data);
  } catch (e) {
    debug('write failed');
  }
}",
        );
        assert!(
            !signals(&f).contains(&"log-only-catch"),
            "{:?}",
            signals(&f)
        );
    }

    /// Without the comment the same code is still reported — the guard must
    /// not silence the signal wherever a file happens to contain comments.
    #[test]
    fn an_undocumented_try_is_still_reported() {
        let f = run_on(
            Language::JavaScript,
            "function send(packet, data) {
  try {
    this.doWrite(packet, data);
  } catch (e) {
    debug('write failed');
  }
}",
        );
        assert!(signals(&f).contains(&"log-only-catch"), "{:?}", signals(&f));
    }

    /// A comment about something further up the function is not about this
    /// handler.
    #[test]
    fn a_distant_comment_does_not_document_the_handler() {
        let f = run_on(
            Language::JavaScript,
            "function send(packet, data) {
  // Prepare the frame before writing.
  const frame = encode(packet);
  const size = frame.length;

  try {
    this.doWrite(frame, data);
  } catch (e) {
    debug('write failed');
  }
}",
        );
        assert!(signals(&f).contains(&"log-only-catch"), "{:?}", signals(&f));
    }

    /// Found by comparing Dross against ruff's BLE001 on the corpus: those two
    /// tools disagreed about exactly nine handlers, and every one of them
    /// recorded the traceback rather than a message.
    #[test]
    fn logging_the_traceback_surfaces_the_error() {
        // scrapy: logger.error(..., exc_info=True)
        let with_exc_info = run_on(
            Language::Python,
            "def close(self):
    try:
        self.slot.close()
    except Exception:
        logger.error('Slot close failure', exc_info=True)
",
        );
        assert!(
            !signals(&with_exc_info).contains(&"overly-broad-catch-type"),
            "{:?}",
            signals(&with_exc_info)
        );

        // scrapy: logger.exception(...)
        let logger_exception = run_on(
            Language::Python,
            "def run(self):
    try:
        self.step()
    except Exception:
        logger.exception('step failed')
",
        );
        assert!(
            !signals(&logger_exception).contains(&"overly-broad-catch-type"),
            "{:?}",
            signals(&logger_exception)
        );

        // A plain message, with no traceback, is still a swallow.
        let message_only = run_on(
            Language::Python,
            "def run(self):
    try:
        self.step()
    except Exception:
        logger.error('step failed')
",
        );
        assert!(
            signals(&message_only).contains(&"overly-broad-catch-type"),
            "{:?}",
            signals(&message_only)
        );
    }

    /// Rust has no catch. What it has is the `Err` arm of a match, which is
    /// where a Result is either dealt with or quietly dropped.
    #[test]
    fn an_empty_error_arm_is_a_swallowed_exception() {
        let f = run_on(
            Language::Rust,
            "fn load(p: &str) -> u32 {
    match read(p) {
        Ok(v) => v,
        Err(_) => {}
    }
    0
}
",
        );
        assert!(
            signals(&f).contains(&"empty-catch-body"),
            "{:?}",
            signals(&f)
        );
    }

    #[test]
    fn an_error_arm_that_hands_the_failure_back_is_not_a_swallow() {
        for body in ["Err(e)", "return Err(e)", "panic!(\"{e}\")"] {
            let src = format!(
                "fn load(p: &str) -> Result<u32, E> {{
    match read(p) {{
        Ok(v) => Ok(v),
        Err(e) => {body},
    }}
}}
"
            );
            let f = run_on(Language::Rust, &src);
            assert!(
                !signals(&f).contains(&"empty-catch-body")
                    && !signals(&f).contains(&"log-only-catch"),
                "{body} should surface: {:?}",
                signals(&f)
            );
        }
    }

    /// Rust logs through macros, which the call-shaped heuristic never saw.
    #[test]
    fn an_error_arm_that_only_logs_is_a_log_only_catch() {
        let f = run_on(
            Language::Rust,
            "fn load(p: &str) -> u32 {
    match read(p) {
        Ok(v) => v,
        Err(e) => {
            warn!(\"read failed: {e}\");
            0
        }
    }
}
",
        );
        assert!(signals(&f).contains(&"log-only-catch"), "{:?}", signals(&f));
    }

    /// The Ok arm is not a handler, and an arm carrying a comment has its
    /// decision written down — both rules the other languages already follow.
    #[test]
    fn ok_arms_and_documented_arms_are_left_alone() {
        let ok_arm = run_on(
            Language::Rust,
            "fn load(p: &str) -> u32 {
    match read(p) {
        Ok(_) => {}
        Err(e) => return handle(e),
    }
    0
}
",
        );
        assert!(signals(&ok_arm).is_empty(), "{:?}", signals(&ok_arm));

        let documented = run_on(
            Language::Rust,
            "fn load(p: &str) -> u32 {
    match read(p) {
        Ok(v) => v,
        Err(_) => {
            // A missing cache entry is the normal case.
        }
    }
    0
}
",
        );
        assert!(
            !signals(&documented).contains(&"empty-catch-body"),
            "{:?}",
            signals(&documented)
        );
    }
}
