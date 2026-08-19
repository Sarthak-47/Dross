//! `dross-bench run` — replays repository history through the engine.

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use dross_core::authorship::{AuthorshipMap, Tag, TaggedRange, tag_from_commit_message};
use dross_core::config::Config;
use dross_core::diff::Repo;
use dross_core::engine::Engine;
use dross_core::finding::Finding;
use dross_core::index::FingerprintIndex;

#[derive(Args)]
pub struct RunArgs {
    /// Repositories to replay. Repeat the flag or pass a directory of clones.
    #[arg(long = "repo", num_args = 1..)]
    repos: Vec<PathBuf>,

    /// Directory containing already-cloned repositories, one per subdirectory.
    #[arg(long)]
    repo_dir: Option<PathBuf>,

    /// Commits to replay per repository, newest first.
    #[arg(long, default_value_t = 300)]
    commits: usize,

    /// Restrict the sample to commits that look agent-authored.
    #[arg(long)]
    agent_only: bool,

    #[arg(long, default_value = ".bench/findings.jsonl")]
    out: PathBuf,
}

/// One finding, carrying the context a labeler needs to judge it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchFinding {
    pub id: String,
    pub repo: String,
    pub commit: String,
    pub commit_summary: String,
    /// Whether the commit carried an agent trailer.
    pub agent_authored: bool,
    pub check: String,
    pub signal: String,
    pub severity: String,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub message: String,
    pub evidence: String,
}

#[derive(Debug, Default, Serialize)]
struct RunStats {
    repos: usize,
    commits_replayed: usize,
    agent_commits: usize,
    findings: usize,
}

pub fn execute(args: RunArgs) -> Result<()> {
    let repos = collect_repos(&args)?;
    anyhow::ensure!(
        !repos.is_empty(),
        "no repositories given; pass --repo or --repo-dir"
    );

    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;

    let mut stats = RunStats {
        repos: repos.len(),
        ..Default::default()
    };

    for repo_path in &repos {
        eprintln!("== {}", repo_path.display());
        match replay(repo_path, &args, &mut out, &mut stats) {
            Ok(n) => eprintln!("   {n} finding(s)"),
            Err(e) => eprintln!("   skipped: {e:#}"),
        }
    }

    eprintln!("\n{}", serde_json::to_string_pretty(&stats)?);
    eprintln!("findings written to {}", args.out.display());
    Ok(())
}

fn collect_repos(args: &RunArgs) -> Result<Vec<PathBuf>> {
    let mut repos = args.repos.clone();
    if let Some(dir) = &args.repo_dir {
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .flatten()
        {
            if entry.path().join(".git").exists() {
                repos.push(entry.path());
            }
        }
    }
    repos.sort();
    Ok(repos)
}

fn replay(
    repo_path: &Path,
    args: &RunArgs,
    out: &mut std::fs::File,
    stats: &mut RunStats,
) -> Result<usize> {
    use std::io::Write;

    let repo = Repo::open(repo_path)?;
    let repo_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.display().to_string());

    // The index is built once per repo and lives inside that repo's own
    // .dross directory, so a benchmark run leaves no state elsewhere.
    let index = FingerprintIndex::open(&Config::index_path(repo_path))?;
    let mut engine = Engine::new(Config::default()).with_index(index);
    engine.build_index(repo_path, |_, _| {})?;
    engine.build_complexity_baseline(repo_path, args.commits)?;

    let mut walker = repo.inner().revwalk()?;
    walker.push_head()?;
    let shas: Vec<String> = walker
        .take(args.commits)
        .filter_map(|r| r.ok())
        .map(|oid| oid.to_string())
        .collect();

    let mut written = 0;
    for pair in shas.windows(2) {
        let (new_sha, old_sha) = (&pair[0], &pair[1]);

        let commit = repo.inner().find_commit(git2::Oid::from_str(new_sha)?).ok();
        let message = commit
            .as_ref()
            .and_then(|c| c.message().ok())
            .unwrap_or("")
            .to_string();
        let summary = message.lines().next().unwrap_or("").to_string();

        let agent = tag_from_commit_message(&message);
        let agent_authored = agent.is_some();
        if args.agent_only && !agent_authored {
            continue;
        }
        if agent_authored {
            stats.agent_commits += 1;
        }

        let Ok(diffs) = repo.diff_commits(old_sha, new_sha) else {
            continue;
        };
        if diffs.is_empty() {
            continue;
        }
        stats.commits_replayed += 1;

        // Tag the whole changed range when the commit names an agent, so
        // authorship-scoped checks run over exactly the intended sample.
        let mut authorship = AuthorshipMap::new();
        if agent_authored {
            for diff in &diffs {
                for hunk in &diff.hunks {
                    if hunk.new_lines == 0 {
                        continue;
                    }
                    authorship.insert(
                        diff.path.clone(),
                        TaggedRange {
                            start_line: hunk.new_start,
                            end_line: hunk.new_start + hunk.new_lines - 1,
                            tag: Tag::Confirmed,
                            reason: "commit trailer names an agent".to_string(),
                        },
                    );
                }
            }
        }

        let report = engine.analyze_diffs(repo_path, &diffs, &authorship)?;
        for (i, finding) in report.findings.iter().enumerate() {
            let record = to_record(&repo_name, new_sha, &summary, agent_authored, i, finding);
            writeln!(out, "{}", serde_json::to_string(&record)?)?;
            written += 1;
            stats.findings += 1;
        }
    }

    Ok(written)
}

fn to_record(
    repo: &str,
    commit: &str,
    summary: &str,
    agent_authored: bool,
    ordinal: usize,
    finding: &Finding,
) -> BenchFinding {
    BenchFinding {
        // Stable across runs so labels survive a re-run of the same sample.
        id: format!(
            "{repo}:{}:{ordinal}:{}",
            &commit[..8.min(commit.len())],
            finding.signal
        ),
        repo: repo.to_string(),
        commit: commit.to_string(),
        commit_summary: summary.to_string(),
        agent_authored,
        check: finding.check.as_str().to_string(),
        signal: finding.signal.clone(),
        severity: format!("{:?}", finding.severity).to_lowercase(),
        file: finding.span.file.display().to_string(),
        start_line: finding.span.start_line,
        end_line: finding.span.end_line,
        message: finding.message.clone(),
        evidence: finding.evidence.clone(),
    }
}
