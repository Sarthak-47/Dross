//! Tautological-test detector (spec section 5).
//!
//! Flags a test whose expected value is derived by re-invoking the same logic
//! under test, rather than a literal or fixture. The canonical agent failure:
//! `expect(slugify(x)).toBe(slugify(x))`.
//!
//! Authorship-specific per spec section 5 — this does not run on untagged
//! human hunks.

use tree_sitter::Node;

use crate::ast::ParsedFile;
use crate::finding::{CheckId, Finding, Severity, SourceSpan};
use crate::lang::Language;

use super::{Check, CheckContext};

pub struct TautologicalTestCheck;

impl Check for TautologicalTestCheck {
    fn id(&self) -> CheckId {
        CheckId::TautologicalTest
    }

    fn applies_to_human_code(&self) -> bool {
        false
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for file in ctx.changed_files() {
            if !is_test_path(&file.path) {
                continue;
            }
            let Some(parsed) = ctx.parsed(&file.path) else {
                continue;
            };
            for assertion in assertions(parsed) {
                if !file.touches_range(assertion.line, assertion.line) {
                    continue;
                }
                findings.extend(evaluate(parsed, &assertion, &file.path));
            }
        }
        findings
    }
}

pub fn is_test_path(path: &std::path::Path) -> bool {
    let s = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    s.contains(".test.")
        || s.contains(".spec.")
        || s.contains("/tests/")
        || s.contains("/test/")
        || s.contains("__tests__")
        || s.rsplit('/').next().is_some_and(|f| f.starts_with("test_"))
        || s.ends_with("_test.py")
}

struct Assertion<'a> {
    node: Node<'a>,
    line: usize,
    /// Text of the actual/subject expression.
    actual: String,
    /// Text of the expected expression, when there is one.
    expected: Option<String>,
}

fn assertions<'a>(file: &'a ParsedFile) -> Vec<Assertion<'a>> {
    let mut out = Vec::new();
    crate::ast::walk(file.root(), |node| {
        let is_call = matches!(node.kind(), "call_expression" | "call");
        if !is_call {
            return;
        }
        if let Some(a) = parse_assertion(file, node) {
            out.push(a);
        }
    });
    out.sort_by_key(|a| a.line);
    out
}

fn parse_assertion<'a>(file: &'a ParsedFile, node: Node<'a>) -> Option<Assertion<'a>> {
    let (line, _) = file.line_span(node);
    match file.language {
        Language::Python => parse_python_assertion(file, node, line),
        _ => parse_js_assertion(file, node, line),
    }
}

/// `expect(actual).toBe(expected)` / `.toEqual(...)` / `assert.equal(a, b)`.
fn parse_js_assertion<'a>(
    file: &'a ParsedFile,
    node: Node<'a>,
    line: usize,
) -> Option<Assertion<'a>> {
    let callee = node.child_by_field_name("function")?;
    let callee_text = file.text(callee);

    // Matcher form: expect(x).toBe(y)
    if callee.kind() == "member_expression" {
        let matcher = callee
            .child_by_field_name("property")
            .map(|p| file.text(p))
            .unwrap_or("");
        if !is_equality_matcher(matcher) {
            return None;
        }
        let object = callee.child_by_field_name("object")?;
        // The object must itself be an expect(...) call.
        if !file.text(object).starts_with("expect") {
            return None;
        }
        let actual = first_argument(file, object)?;
        let expected = first_argument(file, node);
        return Some(Assertion {
            node,
            line,
            actual,
            expected,
        });
    }

    // Node assert form: assert.equal(actual, expected) / assertEquals(a, b)
    let lower = callee_text.to_ascii_lowercase();
    if lower.starts_with("assert") && (lower.contains("equal") || lower.contains("same")) {
        let args = argument_texts(file, node);
        if args.len() >= 2 {
            return Some(Assertion {
                node,
                line,
                actual: args[0].clone(),
                expected: Some(args[1].clone()),
            });
        }
    }
    None
}

/// `self.assertEqual(a, b)` and bare `assert a == b`.
fn parse_python_assertion<'a>(
    file: &'a ParsedFile,
    node: Node<'a>,
    line: usize,
) -> Option<Assertion<'a>> {
    let callee = node.child_by_field_name("function")?;
    let text = file.text(callee).to_ascii_lowercase();
    if !text.contains("assertequal") && !text.contains("assertis") {
        return None;
    }
    let args = argument_texts(file, node);
    if args.len() < 2 {
        return None;
    }
    Some(Assertion {
        node,
        line,
        actual: args[0].clone(),
        expected: Some(args[1].clone()),
    })
}

fn is_equality_matcher(matcher: &str) -> bool {
    matches!(
        matcher,
        "toBe" | "toEqual" | "toStrictEqual" | "toMatchObject" | "toBeCloseTo" | "is" | "deepEqual"
    )
}

fn first_argument(file: &ParsedFile, call: Node<'_>) -> Option<String> {
    argument_texts(file, call).into_iter().next()
}

fn argument_texts(file: &ParsedFile, call: Node<'_>) -> Vec<String> {
    let Some(args) = call.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .map(|c| file.text(c).trim().to_string())
        .collect()
}

fn evaluate(file: &ParsedFile, assertion: &Assertion<'_>, path: &std::path::Path) -> Vec<Finding> {
    let Some(expected) = assertion.expected.as_ref() else {
        return Vec::new();
    };
    let actual = &assertion.actual;
    let span = SourceSpan {
        file: path.to_path_buf(),
        start_line: assertion.line,
        end_line: file.line_span(assertion.node).1,
    };

    // Signal 1: the two sides are textually identical — `expect(f(x)).toBe(f(x))`.
    if normalize_expr(actual) == normalize_expr(expected) && !is_literal(expected) {
        return vec![Finding::new(
            CheckId::TautologicalTest,
            "identical-actual-and-expected",
            Severity::Error,
            span,
            "Test asserts an expression equals itself",
            format!(
                "Both sides of the assertion are `{actual}`, so the test passes for any \
                 implementation and verifies nothing."
            ),
        )];
    }

    // Signal 2: expected re-invokes the same function the actual calls.
    let actual_calls = called_functions(actual);
    let expected_calls = called_functions(expected);
    let shared: Vec<&String> = actual_calls
        .iter()
        .filter(|c| expected_calls.contains(c))
        .collect();

    if !shared.is_empty() && !is_literal(expected) {
        let name = shared[0];
        return vec![Finding::new(
            CheckId::TautologicalTest,
            "expected-derived-from-subject",
            Severity::Error,
            span,
            format!("Expected value is computed by re-invoking `{name}`"),
            format!(
                "The assertion's expected side calls `{name}`, the same logic under test, \
                 instead of comparing against a literal or fixture. The test will pass even \
                 if `{name}` is wrong."
            ),
        )];
    }

    Vec::new()
}

fn normalize_expr(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_literal(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    // Numbers, strings, booleans, and simple collection literals are fine.
    t.parse::<f64>().is_ok()
        || t.starts_with('"')
        || t.starts_with('\'')
        || t.starts_with('`')
        || matches!(
            t,
            "true" | "false" | "null" | "None" | "True" | "False" | "undefined"
        )
        || ((t.starts_with('[') || t.starts_with('{')) && !t.contains('('))
}

/// Extracts callee names from an expression's text. Deliberately syntactic —
/// enough to spot `slugify(x)` on both sides without a resolver.
fn called_functions(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '(' {
            // Walk backwards over the identifier (and dotted path) before `(`.
            let mut j = i;
            while j > 0 {
                let c = bytes[j - 1];
                if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' {
                    j -= 1;
                } else {
                    break;
                }
            }
            if j < i {
                let name: String = bytes[j..i].iter().collect();
                // Keep the last path segment: `utils.slugify` -> `slugify`.
                let leaf = name.rsplit('.').next().unwrap_or(&name).to_string();
                if !leaf.is_empty() && !is_builtin_wrapper(&leaf) {
                    out.push(leaf);
                }
            }
        }
        i += 1;
    }
    out
}

/// Calls that appear on both sides innocently and would cause false positives.
fn is_builtin_wrapper(name: &str) -> bool {
    matches!(
        name,
        "expect"
            | "String"
            | "Number"
            | "Boolean"
            | "Array"
            | "Object"
            | "str"
            | "int"
            | "float"
            | "list"
            | "dict"
            | "len"
            | "toBe"
            | "toEqual"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_on(language: Language, src: &str) -> Vec<Finding> {
        let file = ParsedFile::parse(language, src).unwrap();
        let path = std::path::PathBuf::from("a.test.ts");
        assertions(&file)
            .iter()
            .flat_map(|a| evaluate(&file, a, &path))
            .collect()
    }

    #[test]
    fn flags_expression_equal_to_itself() {
        let f = run_on(
            Language::TypeScript,
            "it('x', () => { expect(slugify(input)).toBe(slugify(input)); });",
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].signal, "identical-actual-and-expected");
    }

    #[test]
    fn flags_expected_derived_from_subject() {
        let f = run_on(
            Language::TypeScript,
            "it('x', () => { expect(slugify('A B')).toBe(slugify('A B').toLowerCase()); });",
        );
        assert_eq!(f.len(), 1, "got {f:?}");
        assert_eq!(f[0].signal, "expected-derived-from-subject");
    }

    #[test]
    fn accepts_a_literal_expectation() {
        let f = run_on(
            Language::TypeScript,
            "it('x', () => { expect(slugify('A B')).toBe('a-b'); });",
        );
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn flags_python_assert_equal_reinvocation() {
        let f = run_on(
            Language::Python,
            "def test_x(self):\n    self.assertEqual(slugify(s), slugify(s))\n",
        );
        assert_eq!(f.len(), 1, "got {f:?}");
    }

    #[test]
    fn accepts_python_literal_expectation() {
        let f = run_on(
            Language::Python,
            "def test_x(self):\n    self.assertEqual(slugify('A B'), 'a-b')\n",
        );
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn recognizes_test_paths() {
        assert!(is_test_path(std::path::Path::new("src/a.test.ts")));
        assert!(is_test_path(std::path::Path::new("tests/test_a.py")));
        assert!(!is_test_path(std::path::Path::new("src/a.ts")));
    }
}
