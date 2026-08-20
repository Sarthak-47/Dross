//! Structural complexity metrics feeding the over-engineering baseline.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::ast::{FunctionDef, ParsedFile};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub node_count: usize,
    /// McCabe cyclomatic complexity: 1 + number of branch points.
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
    node_metrics(node)
}

pub fn node_metrics(node: Node<'_>) -> Metrics {
    let mut node_count = 0;
    let mut branch_points = 0;
    let mut cursor = node.walk();
    let mut stack = vec![(node, 0usize)];
    let mut max_nesting = 0;

    while let Some((n, depth)) = stack.pop() {
        node_count += 1;
        max_nesting = max_nesting.max(depth);
        if is_branch_point(n.kind()) {
            branch_points += 1;
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

fn is_branch_point(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "elif_clause"
            | "else_clause"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "do_statement"
            | "case_statement"
            | "switch_case"
            | "catch_clause"
            | "except_clause"
            | "conditional_expression"
            | "ternary_expression"
            | "logical_expression"
            | "boolean_operator"
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
