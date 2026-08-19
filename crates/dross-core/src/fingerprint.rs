//! Normalized-AST fingerprinting and MinHash, backing clone detection.
//!
//! Normalization is what makes this catch Type-2/3 clones: identifiers and
//! literals are erased, so a renamed-variable copy of an existing function
//! still fingerprints identically.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::ast::{FunctionDef, ParsedFile};

/// Number of MinHash permutations. 128 gives ~±0.09 error on the Jaccard
/// estimate, which is well inside the tolerance for a tunable threshold.
pub const NUM_HASHES: usize = 128;

/// Shingle width over the normalized token stream.
const SHINGLE: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub signature: Vec<u64>,
    /// Distinct shingle count, used to skip trivially small functions.
    pub shingle_count: usize,
}

impl Fingerprint {
    /// Estimated Jaccard similarity, i.e. the fraction of agreeing slots.
    pub fn similarity(&self, other: &Fingerprint) -> f64 {
        if self.signature.len() != other.signature.len() || self.signature.is_empty() {
            return 0.0;
        }
        let agree = self
            .signature
            .iter()
            .zip(&other.signature)
            .filter(|(a, b)| a == b)
            .count();
        agree as f64 / self.signature.len() as f64
    }
}

/// Erases identifiers and literals, keeping structure. `function add(a,b){return a+b}`
/// and `function sum(x,y){return x+y}` normalize to the same token stream.
pub fn normalized_tokens(file: &ParsedFile, func: &FunctionDef) -> Vec<String> {
    let Some(node) = file
        .root()
        .descendant_for_byte_range(func.start_byte, func.end_byte)
    else {
        return Vec::new();
    };

    let mut tokens = Vec::new();
    crate::ast::walk(node, |n| {
        if n.child_count() > 0 {
            // Interior node: the structure itself is the signal.
            tokens.push(n.kind().to_string());
            return;
        }
        let kind = n.kind();
        let token = match kind {
            "identifier" | "property_identifier" | "shorthand_property_identifier"
            | "type_identifier" | "field_identifier" => "@id".to_string(),
            "string" | "template_string" | "string_fragment" => "@str".to_string(),
            "number" | "integer" | "float" => "@num".to_string(),
            "true" | "false" | "null" | "none" | "undefined" => "@lit".to_string(),
            "comment" => return,
            // Operators and keywords are kept verbatim — they carry structure.
            _ => kind.to_string(),
        };
        tokens.push(token);
    });
    tokens
}

fn shingles(tokens: &[String]) -> HashSet<u64> {
    let mut set = HashSet::new();
    if tokens.len() < SHINGLE {
        if !tokens.is_empty() {
            set.insert(hash_str(&tokens.join("\u{1}")));
        }
        return set;
    }
    for window in tokens.windows(SHINGLE) {
        set.insert(hash_str(&window.join("\u{1}")));
    }
    set
}

pub fn fingerprint(file: &ParsedFile, func: &FunctionDef) -> Fingerprint {
    let tokens = normalized_tokens(file, func);
    let shingles = shingles(&tokens);
    Fingerprint {
        signature: minhash(&shingles),
        shingle_count: shingles.len(),
    }
}

fn minhash(shingles: &HashSet<u64>) -> Vec<u64> {
    let mut sig = vec![u64::MAX; NUM_HASHES];
    if shingles.is_empty() {
        return sig;
    }
    for &s in shingles {
        for (i, slot) in sig.iter_mut().enumerate() {
            let h = permute(s, i as u64);
            if h < *slot {
                *slot = h;
            }
        }
    }
    sig
}

/// A cheap, deterministic family of hash permutations (SplitMix64-style
/// finalizer seeded per index). Deterministic across runs and machines, which
/// the reproducibility guarantee requires.
fn permute(value: u64, index: u64) -> u64 {
    let mut x = value
        .wrapping_add(index.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0x165667B19E3779F9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    x
}

fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Language;

    fn fp(src: &str) -> Fingerprint {
        let file = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let func = file.functions().into_iter().next().unwrap();
        fingerprint(&file, &func)
    }

    #[test]
    fn renamed_identifiers_fingerprint_identically() {
        let a = fp("function add(a, b) { const total = a + b; return total; }");
        let b = fp("function sum(x, y) { const result = x + y; return result; }");
        assert!(
            a.similarity(&b) > 0.99,
            "expected near-identical, got {}",
            a.similarity(&b)
        );
    }

    #[test]
    fn structurally_different_functions_diverge() {
        let a = fp("function add(a, b) { return a + b; }");
        let b = fp(
            "function walk(tree) { for (const n of tree.children) { if (n.leaf) { visit(n); } } }",
        );
        assert!(
            a.similarity(&b) < 0.5,
            "expected divergence, got {}",
            a.similarity(&b)
        );
    }

    #[test]
    fn near_duplicate_with_one_extra_statement_stays_similar() {
        let a = fp("function f(a, b) { const t = a + b; return t; }");
        let b = fp("function g(x, y) { const t = x + y; log(t); return t; }");
        let sim = a.similarity(&b);
        assert!(sim > 0.4 && sim < 0.99, "got {sim}");
    }

    #[test]
    fn fingerprints_are_deterministic() {
        let a = fp("function f(a) { return a; }");
        let b = fp("function f(a) { return a; }");
        assert_eq!(a, b);
    }
}
