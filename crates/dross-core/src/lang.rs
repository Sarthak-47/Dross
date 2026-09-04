//! Language detection and tree-sitter grammar loading.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Rust,
}

impl Language {
    /// Detects a language from a file extension. Returns `None` for anything
    /// outside the supported grammars.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "js" | "mjs" | "cjs" | "jsx" => Some(Language::JavaScript),
            "ts" | "mts" | "cts" => Some(Language::TypeScript),
            "tsx" => Some(Language::Tsx),
            "py" | "pyi" => Some(Language::Python),
            "rs" => Some(Language::Rust),
            _ => None,
        }
    }

    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    pub fn parser(self) -> anyhow::Result<tree_sitter::Parser> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.grammar())?;
        Ok(parser)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_extensions() {
        assert_eq!(
            Language::from_path(Path::new("foo/bar.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_path(Path::new("foo/bar.py")),
            Some(Language::Python)
        );
        assert_eq!(
            Language::from_path(Path::new("foo/bar.rs")),
            Some(Language::Rust)
        );
        assert_eq!(Language::from_path(Path::new("foo/bar.go")), None);
    }

    #[test]
    fn parses_a_trivial_rust_snippet() {
        let mut parser = Language::Rust.parser().unwrap();
        let tree = parser
            .parse(
                "fn main() { let x = 1; }
",
                None,
            )
            .unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parses_a_trivial_python_snippet() {
        let mut parser = Language::Python.parser().unwrap();
        let tree = parser.parse("x = 1\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
