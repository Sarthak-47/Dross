//! Normalized-AST fingerprinting and MinHash, backing clone detection.
//!
//! Normalization is what makes this catch Type-2/3 clones: identifiers and
//! literals are erased, so a renamed-variable copy of an existing function
//! still fingerprints identically.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

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
            "identifier"
            | "property_identifier"
            | "shorthand_property_identifier"
            | "type_identifier"
            | "field_identifier" => "@id".to_string(),
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

/// The domain words a function actually names: the properties it reaches for
/// and the functions it calls.
///
/// The normalized token stream deliberately erases these, which is what lets a
/// renamed duplicate match its original. It also erases the only thing that
/// separates a reinvention from deliberate parallel structure.
///
/// The seeded duplicate renames every local — `computeTotal(items)` becomes
/// `sumBasket(entries)` — but still reaches for `.price` and `.quantity`,
/// because it operates on the same data. Two parallel validators, or two
/// adapters implementing one interface, share a shape without sharing any of
/// this vocabulary.
///
/// Parameters and locals are excluded on purpose: those are exactly what gets
/// renamed. Only members and callees survive a rename, so only those are
/// evidence.
pub fn vocabulary(file: &ParsedFile, func: &FunctionDef) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    let Some(node) = file
        .root()
        .descendant_for_byte_range(func.start_byte, func.end_byte)
    else {
        return words;
    };

    crate::ast::walk(node, |n| {
        let keep = match n.kind() {
            // `a.price`, `self.retries`, `obj["k"]` is not included: only names
            // written in the source as members count.
            "property_identifier" | "field_identifier" | "shorthand_property_identifier" => true,
            "identifier" => n.parent().is_some_and(|p| match p.kind() {
                // The callee of a plain call, `helper(x)`. A JavaScript method
                // call's name is already caught as a property above.
                "call_expression" | "call" => true,
                // Python has no property_identifier: `item.price` is an
                // `attribute` node whose object and attribute are both plain
                // identifiers. Only the attribute half is vocabulary — the
                // object is a local, and locals are what get renamed.
                "attribute" => p.child_by_field_name("attribute") == Some(n),
                _ => false,
            }),
            _ => false,
        };
        if !keep {
            return;
        }
        let text = file.text(n).trim();
        // Single characters are almost always loop or lambda variables.
        if text.len() > 1 {
            words.insert(text.to_string());
        }
    });
    words
}

/// Jaccard overlap of two vocabularies, 0.0 when either is empty.
pub fn vocabulary_overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        shared as f64 / union as f64
    }
}

/// Terms two vocabularies have in common.
pub fn shared_vocabulary(a: &BTreeSet<String>, b: &BTreeSet<String>) -> Vec<String> {
    a.intersection(b).cloned().collect()
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

    fn vocab_of(src: &str) -> std::collections::BTreeSet<String> {
        let file = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let func = file.functions().into_iter().next().unwrap();
        vocabulary(&file, &func)
    }

    /// The seeded duplicate: every local renamed, but it still reaches for the
    /// same data, because it operates on the same data.
    #[test]
    fn a_renamed_duplicate_keeps_its_domain_vocabulary() {
        let original = vocab_of(
            "export function computeTotal(items) {
  let total = 0;
  for (const item of items) {
    total += item.price * item.quantity;
  }
  return total;
}",
        );
        let renamed = vocab_of(
            "export function sumBasket(entries) {
  let acc = 0;
  for (const entry of entries) {
    acc += entry.price * entry.quantity;
  }
  return acc;
}",
        );

        assert!(original.contains("price"), "{original:?}");
        assert!(original.contains("quantity"));
        assert_eq!(original, renamed, "a rename must not change the vocabulary");
        assert_eq!(vocabulary_overlap(&original, &renamed), 1.0);
    }

    /// Parallel structure over a different domain: same shape, no shared words.
    /// This is the case that made the signal unusable on real repositories.
    #[test]
    fn parallel_structure_over_another_domain_shares_no_vocabulary() {
        let emails = vocab_of(
            "function validateEmail(input) {
  const parts = input.address.split('@');
  if (parts.length !== 2) { return false; }
  return hasDomain(parts);
}",
        );
        let phones = vocab_of(
            "function validatePhone(input) {
  const digits = input.number.split('-');
  if (digits.length !== 3) { return false; }
  return hasAreaCode(digits);
}",
        );

        assert!(
            vocabulary_overlap(&emails, &phones) < 0.5,
            "{emails:?} vs {phones:?}"
        );
    }

    #[test]
    fn parameters_and_locals_are_excluded_because_they_are_what_gets_renamed() {
        let v = vocab_of(
            "function f(alpha, beta) {
  const gamma = alpha;
  return gamma;
}",
        );
        for local in ["alpha", "beta", "gamma", "f"] {
            assert!(
                !v.contains(local),
                "{local} should not be vocabulary: {v:?}"
            );
        }
    }

    #[test]
    fn callees_count_as_vocabulary() {
        let v = vocab_of(
            "function f(x) {
  return normalise(x) + rescale(x);
}",
        );
        assert!(v.contains("normalise"), "{v:?}");
        assert!(v.contains("rescale"));
    }

    #[test]
    fn overlap_of_an_empty_vocabulary_is_zero() {
        let empty = std::collections::BTreeSet::new();
        let some = vocab_of("function f(x) { return x.value; }");
        assert_eq!(vocabulary_overlap(&empty, &some), 0.0);
        assert_eq!(vocabulary_overlap(&empty, &empty), 0.0);
    }

    /// Python reaches attributes through a different node shape than
    /// JavaScript, and the object half must not be mistaken for vocabulary.
    #[test]
    fn python_attributes_and_calls_are_vocabulary_but_objects_are_not() {
        let src = "def compute_total(items):
    total = 0
    for item in items:
        total += item.price * normalise(item.quantity)
    return total
";
        let file = ParsedFile::parse(Language::Python, src).unwrap();
        let func = file.functions().into_iter().next().unwrap();
        let v = vocabulary(&file, &func);

        assert!(v.contains("price"), "{v:?}");
        assert!(v.contains("quantity"), "{v:?}");
        assert!(v.contains("normalise"), "{v:?}");
        // `item` is the object being reached through, and a local.
        assert!(
            !v.contains("item"),
            "the object half is not vocabulary: {v:?}"
        );
        assert!(!v.contains("items"));
        assert!(!v.contains("total"));
    }

    /// The same logic in the two languages should share vocabulary, so a port
    /// is comparable.
    #[test]
    fn the_same_logic_yields_the_same_vocabulary_in_python_and_javascript() {
        let py = ParsedFile::parse(
            Language::Python,
            "def total(items):
    return sum(i.price for i in items)
",
        )
        .unwrap();
        let js = ParsedFile::parse(
            Language::JavaScript,
            "function total(items) { return sum(items.map((i) => i.price)); }",
        )
        .unwrap();
        let pv = vocabulary(&py, &py.functions()[0]);
        let jv = vocabulary(&js, &js.functions()[0]);
        assert!(
            pv.contains("price") && jv.contains("price"),
            "{pv:?} {jv:?}"
        );
        assert!(pv.contains("sum") && jv.contains("sum"));
    }
}
