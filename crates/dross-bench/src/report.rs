//! `dross-bench report` — scores labeled findings.
//!
//! Reports precision per check and per signal. Recall needs a ground-truth
//! corpus of known-bad code rather than a label pass over emitted findings, so
//! it is reported only when a seeded-defect corpus is supplied, and is
//! otherwise explicitly marked unmeasured rather than silently omitted.

use anyhow::Result;
use clap::Args;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::label::{LabelRow, read_labels};

#[derive(Args)]
pub struct ReportArgs {
    /// Labeled worksheets. Pass twice to compute inter-rater agreement.
    #[arg(long = "labels", num_args = 1..)]
    labels: Vec<PathBuf>,

    /// Emit JSON instead of a text table.
    #[arg(long)]
    json: bool,

    /// Seeded-defect corpus results, enabling recall.
    #[arg(long)]
    seeded: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize)]
struct Score {
    true_positives: usize,
    false_positives: usize,
    unlabeled: usize,
}

impl Score {
    fn labeled(&self) -> usize {
        self.true_positives + self.false_positives
    }

    fn precision(&self) -> Option<f64> {
        if self.labeled() == 0 {
            return None;
        }
        Some(self.true_positives as f64 / self.labeled() as f64)
    }

    /// Wilson score interval — a raw precision from 30 samples without an
    /// interval invites over-reading the number.
    fn confidence_interval(&self) -> Option<(f64, f64)> {
        let n = self.labeled() as f64;
        if n == 0.0 {
            return None;
        }
        let p = self.true_positives as f64 / n;
        let z = 1.96;
        let denom = 1.0 + z * z / n;
        let center = (p + z * z / (2.0 * n)) / denom;
        let margin = z * ((p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt()) / denom;
        Some(((center - margin).max(0.0), (center + margin).min(1.0)))
    }
}

#[derive(Debug, Serialize)]
struct Report {
    by_signal: BTreeMap<String, SignalReport>,
    by_check: BTreeMap<String, SignalReport>,
    overall: SignalReport,
    agreement: Option<Agreement>,
    recall: Option<f64>,
    recall_note: String,
}

#[derive(Debug, Serialize)]
struct SignalReport {
    #[serde(flatten)]
    score: Score,
    precision: Option<f64>,
    ci_low: Option<f64>,
    ci_high: Option<f64>,
}

impl From<Score> for SignalReport {
    fn from(score: Score) -> Self {
        let precision = score.precision();
        let ci = score.confidence_interval();
        Self {
            precision,
            ci_low: ci.map(|c| c.0),
            ci_high: ci.map(|c| c.1),
            score,
        }
    }
}

#[derive(Debug, Serialize)]
struct Agreement {
    compared: usize,
    raw_agreement: f64,
    /// Cohen's kappa, which corrects for agreement expected by chance.
    cohens_kappa: f64,
}

pub fn execute(args: ReportArgs) -> Result<()> {
    anyhow::ensure!(!args.labels.is_empty(), "pass at least one --labels file");

    let passes: Vec<Vec<LabelRow>> = args.labels.iter().map(read_labels).collect::<Result<_>>()?;

    // Scoring uses the first pass; a second pass only contributes agreement.
    let primary = &passes[0];
    let mut by_signal: BTreeMap<String, Score> = BTreeMap::new();
    let mut by_check: BTreeMap<String, Score> = BTreeMap::new();
    let mut overall = Score::default();

    for row in primary {
        let signal = by_signal.entry(row.finding.signal.clone()).or_default();
        let check = by_check.entry(row.finding.check.clone()).or_default();
        for target in [signal, check, &mut overall] {
            match row.verdict.as_deref() {
                Some("tp") => target.true_positives += 1,
                Some("fp") => target.false_positives += 1,
                _ => target.unlabeled += 1,
            }
        }
    }

    let agreement = if passes.len() >= 2 {
        Some(compute_agreement(&passes[0], &passes[1]))
    } else {
        None
    };

    let (recall, recall_note) = match &args.seeded {
        Some(path) => {
            let seeded = read_labels(path)?;
            let found = seeded
                .iter()
                .filter(|r| r.verdict.as_deref() == Some("tp"))
                .count();
            let total = seeded.len();
            (
                if total == 0 {
                    None
                } else {
                    Some(found as f64 / total as f64)
                },
                format!("measured against {total} seeded defects"),
            )
        }
        None => (
            None,
            "not measured — recall requires a seeded-defect corpus, not a label \
             pass over emitted findings"
                .to_string(),
        ),
    };

    let report = Report {
        by_signal: by_signal.into_iter().map(|(k, v)| (k, v.into())).collect(),
        by_check: by_check.into_iter().map(|(k, v)| (k, v.into())).collect(),
        overall: overall.into(),
        agreement,
        recall,
        recall_note,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_table(&report);
    }
    Ok(())
}

fn compute_agreement(a: &[LabelRow], b: &[LabelRow]) -> Agreement {
    let index_b: BTreeMap<&str, Option<&str>> = b
        .iter()
        .map(|r| (r.finding.id.as_str(), r.verdict.as_deref()))
        .collect();

    let mut agreed = 0usize;
    let mut compared = 0usize;
    // Confusion counts for kappa's chance-agreement term.
    let (mut a_tp, mut a_fp, mut b_tp, mut b_fp) = (0usize, 0usize, 0usize, 0usize);

    for row in a {
        let Some(other) = index_b.get(row.finding.id.as_str()) else {
            continue;
        };
        let (Some(x), Some(y)) = (row.verdict.as_deref(), *other) else {
            continue;
        };
        compared += 1;
        if x == y {
            agreed += 1;
        }
        if x == "tp" {
            a_tp += 1;
        } else {
            a_fp += 1;
        }
        if y == "tp" {
            b_tp += 1;
        } else {
            b_fp += 1;
        }
    }

    if compared == 0 {
        return Agreement {
            compared: 0,
            raw_agreement: 0.0,
            cohens_kappa: 0.0,
        };
    }

    let n = compared as f64;
    let po = agreed as f64 / n;
    let pe = (a_tp as f64 / n) * (b_tp as f64 / n) + (a_fp as f64 / n) * (b_fp as f64 / n);
    let kappa = if (1.0 - pe).abs() < f64::EPSILON {
        1.0
    } else {
        (po - pe) / (1.0 - pe)
    };

    Agreement {
        compared,
        raw_agreement: po,
        cohens_kappa: kappa,
    }
}

fn print_table(report: &Report) {
    println!("\nPrecision by signal");
    println!(
        "{:<38} {:>5} {:>5} {:>10} {:>16}",
        "signal", "tp", "fp", "precision", "95% CI"
    );
    for (signal, r) in &report.by_signal {
        print_row(signal, r);
    }

    println!("\nPrecision by check");
    for (check, r) in &report.by_check {
        print_row(check, r);
    }

    println!("\nOverall");
    print_row("all", &report.overall);

    if let Some(a) = &report.agreement {
        println!(
            "\nInter-rater agreement over {} shared items: {:.1}% raw, kappa {:.3}",
            a.compared,
            a.raw_agreement * 100.0,
            a.cohens_kappa
        );
    } else {
        println!("\nInter-rater agreement: not computed (pass a second --labels file)");
    }

    match report.recall {
        Some(r) => println!("Recall: {:.1}% — {}", r * 100.0, report.recall_note),
        None => println!("Recall: {}", report.recall_note),
    }
    println!();
}

fn print_row(name: &str, r: &SignalReport) {
    let precision = r
        .precision
        .map(|p| format!("{:.1}%", p * 100.0))
        .unwrap_or_else(|| "—".to_string());
    let ci = match (r.ci_low, r.ci_high) {
        (Some(lo), Some(hi)) => format!("{:.1}–{:.1}%", lo * 100.0, hi * 100.0),
        _ => "—".to_string(),
    };
    println!(
        "{:<38} {:>5} {:>5} {:>10} {:>16}",
        name, r.score.true_positives, r.score.false_positives, precision, ci
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_is_none_without_labels() {
        let s = Score {
            unlabeled: 5,
            ..Default::default()
        };
        assert!(s.precision().is_none());
    }

    #[test]
    fn confidence_interval_widens_on_small_samples() {
        let small = Score {
            true_positives: 8,
            false_positives: 2,
            unlabeled: 0,
        };
        let large = Score {
            true_positives: 800,
            false_positives: 200,
            unlabeled: 0,
        };
        let (slo, shi) = small.confidence_interval().unwrap();
        let (llo, lhi) = large.confidence_interval().unwrap();
        assert!(
            (shi - slo) > (lhi - llo),
            "small sample should have a wider interval"
        );
    }

    #[test]
    fn kappa_is_one_for_perfect_agreement() {
        let rows = |verdicts: [&str; 4]| -> Vec<LabelRow> {
            verdicts
                .iter()
                .enumerate()
                .map(|(i, v)| LabelRow {
                    finding: crate::run::BenchFinding {
                        id: format!("id-{i}"),
                        repo: "r".into(),
                        commit: "c".into(),
                        commit_summary: "s".into(),
                        agent_authored: true,
                        check: "swallowed-exception".into(),
                        signal: "empty-catch-body".into(),
                        severity: "error".into(),
                        file: "a.js".into(),
                        start_line: 1,
                        end_line: 1,
                        message: "m".into(),
                        evidence: "e".into(),
                    },
                    labeler: "x".into(),
                    verdict: Some(v.to_string()),
                    note: None,
                })
                .collect()
        };
        let a = rows(["tp", "tp", "fp", "fp"]);
        let b = rows(["tp", "tp", "fp", "fp"]);
        let agreement = compute_agreement(&a, &b);
        assert_eq!(agreement.compared, 4);
        assert!((agreement.cohens_kappa - 1.0).abs() < 1e-9);
    }
}
