//! Structural complexity metrics feeding the over-engineering baseline.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::ast::{FunctionDef, ParsedFile, is_function_node};
use crate::lang::Language;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub node_count: usize,
    /// McCabe cyclomatic complexity: 1 + number of decision points.
    ///
    /// The published metric, not a variant of it — checked against ruff's
    /// `C901`, which is the reference `mccabe` implementation, over every
    /// Python function in the benchmark corpus. `.bench/complexityvalidate.py`
    /// reproduces that comparison.
    pub cyclomatic: usize,
    pub max_nesting: usize,
}

pub fn function_metrics(file: &ParsedFile, func: &FunctionDef) -> Metrics {
    let Some(node) = file
        .root()
        .descendant_for_byte_range(func.start_byte, func.end_byte)
    else {
        return Metrics::default();
    };
    node_metrics(file.language, node)
}

pub fn node_metrics(language: Language, node: Node<'_>) -> Metrics {
    let mut node_count = 0;
    let mut branch_points = 0;
    let mut cursor = node.walk();
    let mut stack = vec![(node, 0usize)];
    let mut max_nesting = 0;

    while let Some((n, depth)) = stack.pop() {
        node_count += 1;
        max_nesting = max_nesting.max(depth);
        if is_decision_point(n) {
            branch_points += 1;
        }

        // A nested function is one decision point in this function and nothing
        // more: its own branches belong to its own graph, and the index scores
        // it separately, so descending would count them twice. mccabe draws
        // the boundary in exactly this place.
        if !std::ptr::eq(&n, &node) && n.id() != node.id() && is_function_node(language, n.kind()) {
            branch_points += 1;
            continue;
        }

        let child_depth = if increases_nesting(n.kind()) {
            depth + 1
        } else {
            depth
        };
        for child in n.children(&mut cursor) {
            stack.push((child, child_depth));
        }
    }

    Metrics {
        node_count,
        cyclomatic: branch_points + 1,
        max_nesting,
    }
}

/// Whether this node is a decision in the control flow graph.
///
/// Kind alone answers it everywhere except Python's `match`, where the arm has
/// to be read: `case _` and `case name` always match, so like an `else` they
/// are the default path rather than a decision. `case Foo.bar` is a value
/// pattern and `case name if guard` can fail, so both count.
fn is_decision_point(node: Node<'_>) -> bool {
    if node.kind() == "case_clause" {
        return is_refutable_case(node);
    }
    is_branch_point(node.kind())
}

fn is_refutable_case(clause: Node<'_>) -> bool {
    let mut cursor = clause.walk();
    let children: Vec<Node<'_>> = clause.children(&mut cursor).collect();

    // A guard can fail however the pattern is written.
    if children.iter().any(|c| c.kind() == "if_clause") {
        return true;
    }
    let Some(pattern) = children.iter().find(|c| c.kind() == "case_pattern") else {
        return true;
    };
    let mut inner = pattern.walk();
    let parts: Vec<Node<'_>> = pattern.children(&mut inner).collect();
    let [only] = parts.as_slice() else {
        return true;
    };
    match only.kind() {
        // `case _:`
        "_" => false,
        // `case name:` binds and always matches; `case Enum.member:` compares.
        "dotted_name" => {
            let mut names = only.walk();
            only.children(&mut names)
                .filter(|c| c.kind() == "identifier")
                .count()
                > 1
        }
        _ => true,
    }
}

fn is_branch_point(kind: &str) -> bool {
    matches!(
        kind,
        // `else_clause` is deliberately absent, and so are `&&` / `and` and
        // the ternary operator.
        //
        // An if/else is one decision, not two: nothing in McCabe counts the
        // else arm separately, and counting it inflated every function in
        // proportion to how many else branches it happened to have — which is
        // the worst shape of error for a signal that scores a change against a
        // distribution, because it moves functions around in that distribution
        // for a reason unrelated to complexity. Boolean operators are counted
        // by the *extended* variant of the metric, not by McCabe's, and a
        // ternary is an expression rather than a decision node in the control
        // flow graph McCabe defined. This field claims to be McCabe's, and the
        // point of naming a published metric is that someone else's
        // implementation can check it.
        //
        // Both were found by comparing against ruff's C901 over the corpus.
        "if_statement"
            | "elif_clause"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "do_statement"
            | "case_statement"
            | "switch_case"
            // Python's `match`. Its absence made every match statement in a
            // Python codebase invisible to this metric: a five-arm match
            // scored the same as a straight line.
            | "case_clause"
            | "catch_clause"
            | "except_clause"
            // Rust. `binary_expression` is deliberately absent: it covers
            // arithmetic as well as `&&`/`||`, so counting it would inflate
            // every function that adds two numbers.
            | "if_expression"
            | "match_arm"
            | "for_expression"
            | "while_expression"
            | "loop_expression"
    )
}

fn increases_nesting(kind: &str) -> bool {
    matches!(
        kind,
        "statement_block"
            | "block"
            | "if_statement"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "try_statement"
            | "function_declaration"
            | "function_definition"
            // Rust
            | "if_expression"
            | "for_expression"
            | "while_expression"
            | "loop_expression"
            | "match_expression"
            | "function_item"
    )
}

/// Complexity a change *adds*, rather than the complexity it touches.
///
/// The original formulation summed the full complexity of every function a
/// diff touched, so a repository-wide reformat accumulated the complexity of
/// everything it reformatted. The benchmark caught it firing on commits titled
/// "chore: format", "fix: linting issues", and "chore: fix some comments" — at
/// up to 8.4 standard deviations. Subtracting each function's previous
/// complexity leaves roughly zero for a mechanical change and the real figure
/// for new logic.
///
/// Both the per-change measurement and the baseline it is scored against must
/// use this same function, or the distribution and the sample disagree.
pub fn added_complexity(
    new_parsed: &ParsedFile,
    old_parsed: Option<&ParsedFile>,
    touches: impl Fn(usize, usize) -> bool,
) -> usize {
    use std::collections::HashMap;

    let weigh = |m: Metrics| m.cyclomatic + m.node_count / 10;

    let previous: HashMap<String, usize> = old_parsed
        .map(|old| {
            old.functions()
                .into_iter()
                .filter_map(|f| {
                    let name = f.qualified_name()?;
                    Some((name, weigh(function_metrics(old, &f))))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut added = 0usize;
    for func in new_parsed.functions() {
        if !touches(func.start_line, func.end_line) {
            continue;
        }
        let now = weigh(function_metrics(new_parsed, &func));
        let before = func
            .qualified_name()
            .and_then(|n| previous.get(&n).copied())
            .unwrap_or(0);
        added += now.saturating_sub(before);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lang::Language;

    fn metrics_of(src: &str) -> Metrics {
        let file = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let func = file.functions().into_iter().next().unwrap();
        function_metrics(&file, &func)
    }

    #[test]
    fn reformatting_adds_no_complexity() {
        // Same logic, different formatting: the delta must be zero even though
        // the function itself is complex.
        let old_src = "function f(a){if(a){for(const x of a){g(x);}}return a;}";
        let new_src = "function f(a) {
  if (a) {
    for (const x of a) {
      g(x);
    }
  }
  return a;
}";
        let old = ParsedFile::parse(Language::JavaScript, old_src).unwrap();
        let new = ParsedFile::parse(Language::JavaScript, new_src).unwrap();
        assert_eq!(added_complexity(&new, Some(&old), |_, _| true), 0);
    }

    #[test]
    fn new_logic_counts_as_added_complexity() {
        let old = ParsedFile::parse(Language::JavaScript, "function f(a) { return a; }").unwrap();
        let new = ParsedFile::parse(
            Language::JavaScript,
            "function f(a) { if (a) { for (const x of a) { if (x) { g(x); } } } return a; }",
        )
        .unwrap();
        assert!(added_complexity(&new, Some(&old), |_, _| true) > 0);
    }

    #[test]
    fn an_entirely_new_function_counts_in_full() {
        let new = ParsedFile::parse(
            Language::JavaScript,
            "function f(a) { if (a) { return 1; } return 0; }",
        )
        .unwrap();
        assert!(added_complexity(&new, None, |_, _| true) > 0);
    }

    fn python_complexity(src: &str) -> Vec<(String, usize)> {
        let file = ParsedFile::parse(Language::Python, src).unwrap();
        file.functions()
            .into_iter()
            .map(|f| {
                let name = f.name.clone().unwrap_or_default();
                (name, function_metrics(&file, &f).cyclomatic)
            })
            .collect()
    }

    /// Every figure on the right came from ruff's `C901` — the reference
    /// `mccabe` implementation — run over exactly this source.
    ///
    /// The metric is named after a published paper, so an independent
    /// implementation either agrees or one of us is wrong. Comparing them over
    /// the benchmark corpus found two real faults and moved agreement from 74%
    /// to 97%: `else` was counted as a decision of its own, which nothing in
    /// McCabe does, and Python's `match` was counted as nothing at all, so a
    /// five-arm match scored the same as a straight line.
    ///
    /// `.bench/complexityvalidate.py` reproduces the corpus-wide comparison.
    #[test]
    fn cyclomatic_matches_the_reference_mccabe_implementation() {
        let cases: &[(&str, &str, usize)] = &[
            (
                "straight",
                "def straight(a):
    return a + 1
",
                1,
            ),
            (
                "one_if",
                "def one_if(a):
    if a:
        return 1
    return 0
",
                2,
            ),
            // An if/else is one decision, not two.
            (
                "if_else",
                "def if_else(a):
    if a:
        return 1
    else:
        return 0
",
                2,
            ),
            // Boolean operators belong to the extended variant, not this one.
            (
                "boolean_op",
                "def boolean_op(a, b):
    return a and b
",
                1,
            ),
            // Nor is a ternary a node in McCabe's control flow graph.
            (
                "ternary",
                "def ternary(a):
    return 1 if a else 2
",
                1,
            ),
            (
                "loop_else",
                "def loop_else(a):
    for x in a:
        pass
    else:
        pass
",
                2,
            ),
            (
                "elif_chain",
                "def elif_chain(a):
    if a == 1:
        return 1
    elif a == 2:
        return 2
    elif a == 3:
        return 3
    return 0
",
                4,
            ),
            (
                "match_stmt",
                "def match_stmt(a):
    match a:
        case 1:
            return 1
        case 2:
            return 2
    return 0
",
                3,
            ),
            (
                "multi_except",
                "def multi_except(a):
    try:
        return a()
    except ValueError:
        return 1
    except KeyError:
        return 2
",
                3,
            ),
            (
                "with_stmt",
                "def with_stmt(a):
    with open(a) as f:
        return f.read()
",
                1,
            ),
            (
                "assert_stmt",
                "def assert_stmt(a):
    assert a
    return 1
",
                1,
            ),
            (
                "comprehension_if",
                "def comprehension_if(a):
    return [x for x in a if x]
",
                1,
            ),
            (
                "lambda_arg",
                "def lambda_arg(a):
    return sorted(a, key=lambda x: x.n)
",
                1,
            ),
            // A nested definition is one decision in the enclosing function.
            (
                "outer_plain",
                "def outer_plain(a):
    def inner():
        return 1
    return inner
",
                2,
            ),
        ];

        for (name, src, expected) in cases {
            let got = python_complexity(src);
            let found = got
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("{name} not parsed out of {src:?}"));
            assert_eq!(
                found.1, *expected,
                "{name}: mccabe says {expected}, this says {}",
                found.1
            );
        }
    }

    /// `case _` and `case name` always match, so they are the default path
    /// rather than a decision — the same rule that keeps `else` out of the
    /// count. Every figure here is ruff's.
    ///
    /// Found in black's test corpus, where a function whose whole body is
    /// `match str: case _: pass` scored 2 against mccabe's 1.
    #[test]
    fn an_irrefutable_case_arm_is_not_a_decision() {
        let cases: &[(&str, usize)] = &[
            // Wildcard only: no decision at all.
            (
                "def f(a):
    match a:
        case _:
            pass
",
                1,
            ),
            // A capture binds and always matches.
            (
                "def f(a):
    match a:
        case other:
            pass
",
                1,
            ),
            // One real arm, then a catch-all.
            (
                "def f(a):
    match a:
        case 1:
            pass
        case _:
            pass
",
                2,
            ),
            // A guard can fail, so it is a decision however the pattern reads.
            (
                "def f(a):
    match a:
        case x if x > 1:
            pass
",
                2,
            ),
            // A dotted name is a value pattern, not a capture.
            (
                "def f(a):
    match a:
        case Color.RED:
            pass
",
                2,
            ),
        ];
        for (src, expected) in cases {
            let got = python_complexity(src);
            assert_eq!(
                got[0].1, *expected,
                "mccabe says {expected} for {src:?}, this says {}",
                got[0].1
            );
        }
    }

    /// The one place the two deliberately part company.
    ///
    /// mccabe folds a nested function's decisions into its parent *and*
    /// reports the nested function separately, so the same branches are
    /// counted twice across the two figures it prints. Dross indexes every
    /// function once and scores it once, so the parent gets one point for
    /// containing a definition and nothing more. mccabe returns 4 here.
    ///
    /// This accounts for essentially all of the residual 3% disagreement on
    /// the corpus, and it is a difference of definition rather than a fault.
    #[test]
    fn a_nested_functions_own_branches_are_not_charged_to_its_parent() {
        let src = "def outer(a):
    def inner(b):
        if b:
            return 1
        for x in b:
            pass
        return 0
    return inner
";
        let got = python_complexity(src);
        let outer = got.iter().find(|(n, _)| n == "outer").unwrap();
        let inner = got.iter().find(|(n, _)| n == "inner").unwrap();
        assert_eq!(outer.1, 2, "one point for the definition, none of its body");
        assert_eq!(inner.1, 3, "charged in full to the function that has them");
    }

    #[test]
    fn straight_line_function_has_complexity_one() {
        let m = metrics_of("function f(a) { return a + 1; }");
        assert_eq!(m.cyclomatic, 1);
    }

    #[test]
    fn branches_raise_cyclomatic_complexity() {
        let m = metrics_of(
            "function f(a) { if (a) { return 1; } for (const x of a) { g(x); } return 0; }",
        );
        assert!(m.cyclomatic >= 3, "got {}", m.cyclomatic);
    }
}
