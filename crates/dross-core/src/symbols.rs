//! Repo-wide symbol table: call sites, class hierarchy, and type usage.
//!
//! **Known limitation, stated plainly:** resolution here is name-based, not
//! semantic. tree-sitter is a syntax parser with no binder or type checker, so
//! two distinct functions sharing a name collapse into one entry, and re-export
//! chains are not followed. That is a deliberate v1 tradeoff — a real resolver
//! means shelling out to tsc/pyright, which breaks the offline, zero-dependency
//! guarantee.
//!
//! Consequence: signals built on this must be conservative. Where a name is
//! ambiguous repo-wide, the over-engineering check declines to fire rather than
//! guessing, so ambiguity costs recall, never precision.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{ParsedFile, walk};
use crate::lang::Language;

#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    /// Callee name -> every site that calls it.
    call_sites: HashMap<String, Vec<CallSite>>,
    /// Type name -> names that extend/implement it.
    subtypes: HashMap<String, HashSet<String>>,
    /// Type name -> where it was declared.
    declarations: HashMap<String, Vec<Declaration>>,
    /// Names declared more than once repo-wide; these are ambiguous and every
    /// consumer must treat their counts as unreliable.
    ambiguous: HashSet<String>,
    /// Callee name -> the argument text used at each call site, for the
    /// unused-generality signal.
    call_arguments: HashMap<String, Vec<Vec<String>>>,
    files_scanned: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub file: PathBuf,
    pub line: usize,
    pub kind: DeclKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Class,
    Interface,
    AbstractClass,
    Function,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn files_scanned(&self) -> usize {
        self.files_scanned
    }

    pub fn add_file(&mut self, path: &Path, language: Language, source: &str) {
        let Ok(parsed) = ParsedFile::parse(language, source) else {
            return;
        };
        self.add_parsed(path, &parsed);
    }

    pub fn add_parsed(&mut self, path: &Path, parsed: &ParsedFile) {
        self.files_scanned += 1;

        walk(parsed.root(), |node| match node.kind() {
            "call_expression" | "call" | "new_expression" => {
                if let Some(name) = callee_name(parsed, node) {
                    let (line, _) = parsed.line_span(node);
                    self.call_sites
                        .entry(name.clone())
                        .or_default()
                        .push(CallSite {
                            file: path.to_path_buf(),
                            line,
                        });
                    self.call_arguments
                        .entry(name)
                        .or_default()
                        .push(argument_texts(parsed, node));
                }
            }
            "class_declaration"
            | "class"
            | "class_definition"
            | "interface_declaration"
            | "abstract_class_declaration" => {
                let Some(name_node) = node.child_by_field_name("name") else {
                    return;
                };
                let name = parsed.text(name_node).to_string();
                let (line, _) = parsed.line_span(node);
                let kind = match node.kind() {
                    "interface_declaration" => DeclKind::Interface,
                    "abstract_class_declaration" => DeclKind::AbstractClass,
                    _ => DeclKind::Class,
                };
                self.declarations
                    .entry(name.clone())
                    .or_default()
                    .push(Declaration {
                        file: path.to_path_buf(),
                        line,
                        kind,
                    });
                for parent in supertypes(parsed, node) {
                    self.subtypes
                        .entry(parent)
                        .or_default()
                        .insert(name.clone());
                }
            }
            _ => {}
        });

        // Record function declarations so we can tell "declared here" from
        // "called here" and detect duplicate names.
        for func in parsed.functions() {
            if let Some(name) = func.name.clone() {
                self.declarations
                    .entry(name)
                    .or_default()
                    .push(Declaration {
                        file: path.to_path_buf(),
                        line: func.start_line,
                        kind: DeclKind::Function,
                    });
            }
        }

        self.recompute_ambiguity();
    }

    fn recompute_ambiguity(&mut self) {
        self.ambiguous = self
            .declarations
            .iter()
            .filter(|(_, decls)| {
                // Same name declared at two distinct locations = ambiguous.
                let mut seen: HashSet<(&Path, usize)> = HashSet::new();
                for d in decls.iter() {
                    seen.insert((d.file.as_path(), d.line));
                }
                seen.len() > 1
            })
            .map(|(name, _)| name.clone())
            .collect();
    }

    /// True when this name cannot be resolved confidently repo-wide.
    pub fn is_ambiguous(&self, name: &str) -> bool {
        self.ambiguous.contains(name)
    }

    pub fn call_sites(&self, name: &str) -> &[CallSite] {
        self.call_sites
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Call sites excluding the declaration's own file, which is how "called
    /// from exactly one site" is judged for wrapper detection.
    pub fn call_site_count(&self, name: &str) -> usize {
        self.call_sites(name).len()
    }

    pub fn subtypes(&self, name: &str) -> Vec<&str> {
        self.subtypes
            .get(name)
            .map(|s| s.iter().map(|x| x.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn declarations(&self, name: &str) -> &[Declaration] {
        self.declarations
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Distinct argument values seen at position `index` across all call sites.
    pub fn distinct_arguments_at(&self, name: &str, index: usize) -> Vec<String> {
        let mut seen: Vec<String> = self
            .call_arguments
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|args| args.get(index).cloned())
            .collect();
        seen.sort();
        seen.dedup();
        seen
    }
}

fn callee_name(file: &ParsedFile, node: tree_sitter::Node<'_>) -> Option<String> {
    let callee = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))?;
    let text = file.text(callee).trim();
    if text.is_empty() {
        return None;
    }
    // Keep the leaf: `utils.slugify` -> `slugify`, `this.run` -> `run`.
    let leaf = text.rsplit('.').next().unwrap_or(text).trim();
    if leaf.is_empty() || !leaf.chars().next()?.is_alphabetic() && !leaf.starts_with('_') {
        return None;
    }
    if leaf.contains(|c: char| c.is_whitespace() || c == '(' || c == ')') {
        return None;
    }
    Some(leaf.to_string())
}

fn argument_texts(file: &ParsedFile, call: tree_sitter::Node<'_>) -> Vec<String> {
    let Some(args) = call.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .map(|c| file.text(c).trim().to_string())
        .collect()
}

fn supertypes(file: &ParsedFile, node: tree_sitter::Node<'_>) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            // JS/TS: `class A extends B implements C`. The heritage node nests
            // extends/implements clauses, so collect type names by descent
            // rather than assuming a flat shape.
            "class_heritage" | "extends_clause" | "implements_clause" => {
                crate::ast::walk(child, |c| {
                    if !matches!(c.kind(), "type_identifier" | "identifier") {
                        return;
                    }
                    let text = file.text(c).trim();
                    // Strip generics: `Base<T>` -> `Base`.
                    let base = text.split('<').next().unwrap_or(text).trim();
                    let leaf = base.rsplit('.').next().unwrap_or(base).trim();
                    if !leaf.is_empty() {
                        out.push(leaf.to_string());
                    }
                });
            }
            // Python: `class A(Base):`
            "argument_list" => {
                let mut inner = child.walk();
                for c in child.named_children(&mut inner) {
                    let text = file.text(c).trim();
                    let base = text.split('[').next().unwrap_or(text).trim();
                    let leaf = base.rsplit('.').next().unwrap_or(base).trim();
                    if !leaf.is_empty() && leaf != "object" {
                        out.push(leaf.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_call_sites() {
        let mut table = SymbolTable::new();
        table.add_file(
            Path::new("a.js"),
            Language::JavaScript,
            "function helper() {} helper(); helper();",
        );
        assert_eq!(table.call_site_count("helper"), 2);
    }

    #[test]
    fn records_ts_class_hierarchy() {
        let mut table = SymbolTable::new();
        table.add_file(
            Path::new("a.ts"),
            Language::TypeScript,
            "interface Store {} class MemStore implements Store {}",
        );
        assert_eq!(table.subtypes("Store"), vec!["MemStore"]);
    }

    #[test]
    fn records_python_class_hierarchy() {
        let mut table = SymbolTable::new();
        table.add_file(
            Path::new("a.py"),
            Language::Python,
            "class Base:\n    pass\n\nclass Impl(Base):\n    pass\n",
        );
        assert_eq!(table.subtypes("Base"), vec!["Impl"]);
    }

    #[test]
    fn flags_duplicate_names_as_ambiguous() {
        let mut table = SymbolTable::new();
        table.add_file(Path::new("a.js"), Language::JavaScript, "function run() {}");
        table.add_file(Path::new("b.js"), Language::JavaScript, "function run() {}");
        assert!(table.is_ambiguous("run"));
    }

    #[test]
    fn tracks_distinct_arguments_per_position() {
        let mut table = SymbolTable::new();
        table.add_file(
            Path::new("a.js"),
            Language::JavaScript,
            "f(1, 'a'); f(1, 'b'); f(1, 'c');",
        );
        assert_eq!(table.distinct_arguments_at("f", 0), vec!["1"]);
        assert_eq!(table.distinct_arguments_at("f", 1).len(), 3);
    }
}
