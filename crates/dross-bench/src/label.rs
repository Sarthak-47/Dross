//! `dross-bench label` — builds a labeling worksheet.
//!
//! Sampling is stratified per signal rather than uniform: the rare signals are
//! exactly the ones whose precision is least certain, and a uniform sample of
//! a findings set dominated by one signal would leave them unmeasured.

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use crate::run::BenchFinding;

#[derive(Args)]
pub struct LabelArgs {
    #[arg(long, default_value = ".bench/findings.jsonl")]
    findings: PathBuf,

    #[arg(long, default_value = ".bench/worksheet.jsonl")]
    out: PathBuf,

    /// Findings sampled per signal.
    #[arg(long, default_value_t = 30)]
    per_signal: usize,

    /// Name recorded as the labeler, so two passes can be compared.
    #[arg(long, default_value = "labeler-1")]
    labeler: String,

    /// Deterministic sampling seed, so a worksheet can be regenerated exactly.
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelRow {
    #[serde(flatten)]
    pub finding: BenchFinding,
    pub labeler: String,
    /// Filled in by hand: "tp" (real issue), "fp" (false positive), or null.
    pub verdict: Option<String>,
    /// Optional free-text justification, useful when adjudicating disagreement.
    pub note: Option<String>,
}

pub fn execute(args: LabelArgs) -> Result<()> {
    let findings = read_findings(&args.findings)?;
    anyhow::ensure!(!findings.is_empty(), "no findings to label");

    let mut by_signal: BTreeMap<String, Vec<BenchFinding>> = BTreeMap::new();
    for finding in findings {
        by_signal
            .entry(finding.signal.clone())
            .or_default()
            .push(finding);
    }

    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(&args.out)?;
    let mut total = 0;

    for (signal, mut group) in by_signal {
        // Deterministic shuffle: same seed and same input give the same
        // worksheet, so a published sample is reproducible.
        deterministic_shuffle(&mut group, args.seed);
        let take = args.per_signal.min(group.len());
        eprintln!("{signal}: sampling {take} of {}", group.len());

        for finding in group.into_iter().take(take) {
            let row = LabelRow {
                finding,
                labeler: args.labeler.clone(),
                verdict: None,
                note: None,
            };
            writeln!(out, "{}", serde_json::to_string(&row)?)?;
            total += 1;
        }
    }

    eprintln!("\n{total} row(s) written to {}", args.out.display());
    eprintln!("Fill in \"verdict\" with \"tp\" or \"fp\", then run `dross-bench report`.");
    Ok(())
}

pub fn read_findings(path: &PathBuf) -> Result<Vec<BenchFinding>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).context("parsing findings line"))
        .collect()
}

pub fn read_labels(path: &PathBuf) -> Result<Vec<LabelRow>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).context("parsing worksheet line"))
        .collect()
}

/// FNV-seeded Fisher-Yates. Avoids an rng dependency and keeps the sample
/// reproducible from the seed alone.
fn deterministic_shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_is_deterministic_for_a_seed() {
        let mut a: Vec<u32> = (0..50).collect();
        let mut b: Vec<u32> = (0..50).collect();
        deterministic_shuffle(&mut a, 7);
        deterministic_shuffle(&mut b, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn shuffle_differs_between_seeds() {
        let mut a: Vec<u32> = (0..50).collect();
        let mut b: Vec<u32> = (0..50).collect();
        deterministic_shuffle(&mut a, 7);
        deterministic_shuffle(&mut b, 8);
        assert_ne!(a, b);
    }

    #[test]
    fn shuffle_preserves_every_element() {
        let mut a: Vec<u32> = (0..50).collect();
        deterministic_shuffle(&mut a, 3);
        a.sort();
        assert_eq!(a, (0..50).collect::<Vec<_>>());
    }
}
