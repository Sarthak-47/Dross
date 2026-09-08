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

    /// Whether this path is source code in a language Dross has no grammar for.
    ///
    /// Only used to tell the user what a run did not cover, so it lists
    /// programming languages and nothing else. Reporting every unparsed file
    /// listed `README.md`, `Cargo.lock` and `.gitignore` as "not read", which
    /// is true and useless: a note that fires on every commit is one the
    /// reader learns to skip, and then it is not there on the commit that adds
    /// a `.go` file.
    ///
    /// An extension missing from this list is silence, not a false claim of
    /// coverage — `files_analyzed` counts only what was parsed either way.
    pub fn is_unsupported_source(path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return false;
        };
        matches!(
            ext,
            "go" | "java"
                | "kt"
                | "kts"
                | "scala"
                | "rb"
                | "php"
                | "cs"
                | "c"
                | "h"
                | "cc"
                | "cpp"
                | "cxx"
                | "hpp"
                | "hh"
                | "swift"
                | "m"
                | "mm"
                | "dart"
                | "ex"
                | "exs"
                | "erl"
                | "hs"
                | "ml"
                | "clj"
                | "cljs"
                | "lua"
                | "pl"
                | "pm"
                | "r"
                | "jl"
                | "zig"
                | "nim"
                | "vue"
                | "svelte"
        )
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
