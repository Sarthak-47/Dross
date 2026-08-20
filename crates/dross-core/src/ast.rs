//! Shared AST utilities: parsing, function extraction, and normalization.
//!
//! Every check works from `FunctionDef` rather than raw tree-sitter nodes, so
//! per-language node-name differences are handled once, here.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use crate::lang::Language;

/// A parsed source file, kept alongside its text so nodes stay resolvable.
pub struct ParsedFile {
    pub language: Language,
    pub source: String,
    pub tree: Tree,
}

impl ParsedFile {
    pub fn parse(language: Language, source: impl Into<String>) -> Result<Self> {
        let source = source.into();
        let mut parser = language.parser()?;
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter failed to parse source"))?;
        Ok(Self {
            language,
            source,
            tree,
        })
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    pub fn text(&self, node: Node<'_>) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// 1-indexed line range for a node, matching how diffs and editors count.
    pub fn line_span(&self, node: Node<'_>) -> (usize, usize) {
        (node.start_position().row + 1, node.end_position().row + 1)
    }

    /// Every function/method/closure definition in the file.
    pub fn functions(&self) -> Vec<FunctionDef> {
        let mut out = Vec::new();
        let mut cursor = self.root().walk();
        let mut stack = vec![self.root()];
        while let Some(node) = stack.pop() {
            if is_function_node(self.language, node.kind()) {
                out.push(self.function_at(node));
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        out.sort_by_key(|f| f.start_line);
        out
    }

    fn function_at(&self, node: Node<'_>) -> FunctionDef {
        let (start_line, end_line) = self.line_span(node);
        FunctionDef {
            name: self.function_name(node),
            kind: classify_function(self.language, node.kind()),
            start_line,
            end_line,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            params: self.params_of(node),
            return_type: self.return_type_of(node),
            is_async: self.is_async(node),
            enclosing_type: self.enclosing_type_name(node),
        }
    }

    fn function_name(&self, node: Node<'_>) -> Option<String> {
        if let Some(name) = node.child_by_field_name("name") {
            return Some(self.text(name).to_string());
        }
        // Arrow functions and function expressions borrow the name of the
        // binding they're assigned to: `const foo = () => {}`.
        let parent = node.parent()?;
        match parent.kind() {
            "variable_declarator" | "assignment_expression" | "public_field_definition" => parent
                .child_by_field_name("name")
                .or_else(|| parent.child_by_field_name("left"))
                .map(|n| self.text(n).to_string()),
            "pair" => parent
                .child_by_field_name("key")
                .map(|n| self.text(n).to_string()),
            _ => None,
        }
    }

    fn params_of(&self, node: Node<'_>) -> Vec<Param> {
        let Some(params) = node
            .child_by_field_name("parameters")
            .or_else(|| node.child_by_field_name("parameter"))
        else {
            return Vec::new();
        };
        let mut cursor = params.walk();
        let mut out = Vec::new();
        for child in params.named_children(&mut cursor) {
            if matches!(child.kind(), "comment") {
                continue;
            }
            out.push(self.param_of(child));
        }
        out
    }

    fn param_of(&self, node: Node<'_>) -> Param {
        let kind = node.kind();
        let optional = kind == "optional_parameter"
            || kind == "default_parameter"
            || self.text(node).contains('?')
            || self.text(node).contains('=');
        let variadic = kind.contains("rest")
            || self.text(node).starts_with("...")
            || self.text(node).starts_with('*');
        // Python's `typed_parameter` exposes no name field, so falling back
        // to the node's full text yielded names like "ctx: AppContext".
        // That made every parameter unique and broke name-based alignment
        // between two revisions.
        let name = node
            .child_by_field_name("pattern")
            .or_else(|| node.child_by_field_name("name"))
            .map(|n| self.text(n).to_string())
            .unwrap_or_else(|| {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|c| c.kind() == "identifier")
                    .map(|c| self.text(c).to_string())
                    .unwrap_or_else(|| {
                        // Last resort: text up to the annotation or default.
                        let text = self.text(node);
                        text.split([':', '='])
                            .next()
                            .unwrap_or(text)
                            .trim_start_matches('*')
                            .trim()
                            .to_string()
                    })
            });
        let ty = node
            .child_by_field_name("type")
            .map(|n| normalize_type(self.text(n)));
        Param {
            name: name.trim().to_string(),
            ty,
            optional,
            variadic,
        }
    }

    fn return_type_of(&self, node: Node<'_>) -> Option<String> {
        node.child_by_field_name("return_type")
            .map(|n| normalize_type(self.text(n)))
    }

    fn is_async(&self, node: Node<'_>) -> bool {
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|c| c.kind() == "async")
    }

    fn enclosing_type_name(&self, node: Node<'_>) -> Option<String> {
        let mut current = node.parent();
        while let Some(n) = current {
            if matches!(
                n.kind(),
                "class_declaration" | "class" | "class_definition" | "interface_declaration"
            ) {
                return n
                    .child_by_field_name("name")
                    .map(|x| self.text(x).to_string());
            }
            current = n.parent();
        }
        None
    }

    /// Finds the node covering a byte range, useful for mapping a finding back
    /// to the smallest enclosing construct.
    pub fn node_at_byte(&self, byte: usize) -> Option<Node<'_>> {
        self.root().descendant_for_byte_range(byte, byte)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: Option<String>,
    pub optional: bool,
    pub variadic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FunctionKind {
    Function,
    Method,
    Arrow,
    Constructor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: Option<String>,
    pub kind: FunctionKind,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub is_async: bool,
    pub enclosing_type: Option<String>,
}

impl FunctionDef {
    /// Fully-qualified-ish name used to match a function across two revisions.
    pub fn qualified_name(&self) -> Option<String> {
        let name = self.name.as_ref()?;
        Some(match &self.enclosing_type {
            Some(ty) => format!("{ty}.{name}"),
            None => name.clone(),
        })
    }

    pub fn line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }
}

pub fn is_function_node(language: Language, kind: &str) -> bool {
    match language {
        Language::Python => matches!(kind, "function_definition"),
        // Note: the bare `function` keyword is itself a node of kind
        // "function" in this grammar, so it must not be listed here or every
        // declaration counts twice.
        _ => matches!(
            kind,
            "function_declaration"
                | "function_expression"
                | "arrow_function"
                | "method_definition"
                | "generator_function"
                | "generator_function_declaration"
        ),
    }
}

fn classify_function(language: Language, kind: &str) -> FunctionKind {
    match language {
        Language::Python => FunctionKind::Function,
        _ => match kind {
            "arrow_function" => FunctionKind::Arrow,
            "method_definition" => FunctionKind::Method,
            _ => FunctionKind::Function,
        },
    }
}

/// Strips syntactic noise so `: string` and `:string` compare equal.
pub fn normalize_type(raw: &str) -> String {
    let collapsed = raw
        .trim()
        .trim_start_matches("->")
        .trim_start_matches(':')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = collapsed.trim();

    // Union members are a set, not a sequence. pytest reordering
    // `ModuleType | str | Sequence[str] | None` to
    // `None | ModuleType | str | Sequence[str]` changed nothing a caller can
    // observe, but positional comparison reported it as a type change. Sorting
    // the members makes the two forms compare equal.
    //
    // Splitting is depth-aware so a `|` inside `dict[str, A | B]` does not
    // split the outer type.
    if let Some(members) = split_union(collapsed) {
        let mut parts: Vec<&str> = members.iter().map(|m| m.trim()).collect();
        parts.sort_unstable();
        parts.dedup();
        return parts.join(" | ");
    }
    collapsed.to_string()
}

/// Splits a top-level union, or returns `None` when the type is not one.
fn split_union(text: &str) -> Option<Vec<&str>> {
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        match ch {
            '[' | '(' | '<' | '{' => depth += 1,
            ']' | ')' | '>' | '}' => depth -= 1,
            '|' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(&text[start..]);
    Some(parts)
}

/// Depth-first walk yielding every node, used by the pattern-matching checks.
pub fn walk<'a>(root: Node<'a>, mut visit: impl FnMut(Node<'a>)) {
    let mut cursor = root.walk();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        visit(node);
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: reordering the members of a union is not a contract change.
    /// pytest did exactly that across several signatures and each one was
    /// reported as a type change.
    #[test]
    fn union_members_compare_as_a_set() {
        assert_eq!(
            normalize_type("ModuleType | str | Sequence[str] | None"),
            normalize_type("None | ModuleType | str | Sequence[str]")
        );
        assert_eq!(
            normalize_type("str | NotSetType | None"),
            normalize_type("str | None | NotSetType")
        );
        // A genuinely different union must still differ.
        assert_ne!(normalize_type("str | None"), normalize_type("int | None"));
        // A `|` nested inside a generic must not split the outer type.
        assert_eq!(
            normalize_type("dict[str, A | B]"),
            normalize_type("dict[str, A | B]")
        );
        assert_ne!(
            normalize_type("dict[str, A | B]"),
            normalize_type("dict[str, A]")
        );
    }

    #[test]
    fn extracts_typescript_function_signature() {
        let src = "export function add(a: number, b?: string): number { return 1; }";
        let file = ParsedFile::parse(Language::TypeScript, src).unwrap();
        let funcs = file.functions();
        assert_eq!(funcs.len(), 1);
        let f = &funcs[0];
        assert_eq!(f.name.as_deref(), Some("add"));
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].ty.as_deref(), Some("number"));
        assert!(f.params[1].optional);
        assert_eq!(f.return_type.as_deref(), Some("number"));
    }

    #[test]
    fn python_parameter_names_exclude_their_annotation() {
        let src = "def f(self, ctx: AppContext, context: dict[str, t.Any] = None):
    return ctx
";
        let file = ParsedFile::parse(Language::Python, src).unwrap();
        let f = &file.functions()[0];
        let names: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["self", "ctx", "context"]);
        assert_eq!(f.params[1].ty.as_deref(), Some("AppContext"));
    }

    #[test]
    fn extracts_python_function_and_method() {
        let src = "class A:\n    def m(self, x):\n        return x\n\ndef top(y):\n    return y\n";
        let file = ParsedFile::parse(Language::Python, src).unwrap();
        let funcs = file.functions();
        assert_eq!(funcs.len(), 2);
        let m = funcs
            .iter()
            .find(|f| f.name.as_deref() == Some("m"))
            .unwrap();
        assert_eq!(m.enclosing_type.as_deref(), Some("A"));
        assert_eq!(m.qualified_name().as_deref(), Some("A.m"));
    }

    #[test]
    fn names_arrow_functions_from_their_binding() {
        let src = "const handler = (req) => { return req; };";
        let file = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let funcs = file.functions();
        assert_eq!(funcs[0].name.as_deref(), Some("handler"));
        assert_eq!(funcs[0].kind, FunctionKind::Arrow);
    }
}
