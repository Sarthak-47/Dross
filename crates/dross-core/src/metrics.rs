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
    fn straight_line_function_has_complexity_one() {
        let m = metrics_of("function f(a) { return a + 1; }");
        assert_eq!(m.cyclomatic, 1);
    }

    #[test]
    fn branches_raise_cyclomatic_complexity() {
        let m = metrics_of("function f(a) { if (a) { return 1; } for (const x of a) { g(x); } return 0; }");
        assert!(m.cyclomatic >= 3, "got {}", m.cyclomatic);
    }
}
