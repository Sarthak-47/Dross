//! Dross CLI — the headless/CI surface over the same core the app uses.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use std::path::{Path, PathBuf};

use dross_core::authorship::AuthorshipMap;
use dross_core::config::Config;
use dross_core::diff::DiffTarget;
use dross_core::engine::{Engine, Report};
use dross_core::finding::Severity;
use dross_core::index::FingerprintIndex;

mod render;

#[derive(Parser)]
#[command(
    name = "dross",
    version,
    about = "Catches what agent-generated diffs get wrong, before you commit.",
    long_about = "Dross runs deterministic structural checks over a diff. Every check is a \
                  parser, a hash, or a graph algorithm — there are no model calls, so runs \
                  are reproducible, offline, and free."
)]
struct Cli {
    /// Repository to analyze. Defaults to the current directory.
    #[arg(long, short = 'C', global = true)]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze the current diff.
    Check(CheckArgs),
    /// Build or rebuild the whole-repo fingerprint index.
    Index(IndexArgs),
    /// Show, install, or remove tool integrations.
    Connections(ConnectionsArgs),
    /// Show the risk-history trend recorded by previous runs.
    History(HistoryArgs),
    /// Write a default .dross.json into the repository.
    Init,
}

#[derive(clap::Args)]
struct CheckArgs {
    /// Analyze staged changes (the pre-commit case).
    #[arg(long)]
    staged: bool,
    /// Analyze the working tree instead of the index.
    #[arg(long, conflicts_with = "staged")]
    worktree: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    /// Print only the one-line summary.
    #[arg(long)]
    quiet: bool,
    /// Exit non-zero when findings reach the blocking severity.
    #[arg(long)]
    hook: bool,
    /// Severity at or above which --hook exits non-zero. Overrides the
    /// repository's `block_at` setting; defaults to `error` if neither is set.
    #[arg(long, value_enum)]
    block_at: Option<SeverityArg>,
    /// Skip appending this run to the local risk history.
    #[arg(long)]
    no_record: bool,
    /// No-op unless the invoking command was a `git commit`. Used by the
    /// Codex and Claude Code Bash-tool hooks, which fire on every command.
    #[arg(long)]
    if_git_commit: bool,
}

#[derive(clap::Args)]
struct IndexArgs {
    /// Also replay commit history to build the complexity baseline.
    #[arg(long, default_value_t = true)]
    baseline: bool,
    /// Commits to replay when building the baseline.
    #[arg(long, default_value_t = 200)]
    baseline_commits: usize,
}

#[derive(clap::Args)]
struct ConnectionsArgs {
    #[command(subcommand)]
    action: Option<ConnectionAction>,
}

#[derive(Subcommand)]
enum ConnectionAction {
    /// List detected tools and whether Dross is wired in (default).
    List,
    /// Wire Dross into a tool.
    Install { tool: String },
    /// Remove Dross's wiring from a tool.
    Uninstall { tool: String },
}

#[derive(clap::Args)]
struct HistoryArgs {
    #[arg(long, default_value_t = 20)]
    limit: usize,
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
    /// One line per finding, `file:line: message` — greppable, editor-friendly.
    Compact,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum SeverityArg {
    Info,
    Warning,
    Error,
}

impl From<SeverityArg> for Severity {
    fn from(value: SeverityArg) -> Self {
        match value {
            SeverityArg::Info => Severity::Info,
            SeverityArg::Warning => Severity::Warning,
            SeverityArg::Error => Severity::Error,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let repo_root = match resolve_repo(cli.repo.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} {e:#}", "error:".red().bold());
            std::process::exit(2);
        }
    };

    let code = match run(&cli.command, &repo_root) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e:#}", "error:".red().bold());
            2
        }
    };
    std::process::exit(code);
}

fn run(command: &Command, repo_root: &Path) -> Result<i32> {
    match command {
        Command::Check(args) => cmd_check(args, repo_root),
        Command::Index(args) => cmd_index(args, repo_root),
        Command::Connections(args) => cmd_connections(args, repo_root),
        Command::History(args) => cmd_history(args, repo_root),
        Command::Init => cmd_init(repo_root),
    }
}

fn cmd_check(args: &CheckArgs, repo_root: &Path) -> Result<i32> {
    // Bash-tool hooks fire on every command; only act on commits.
    if args.if_git_commit && !invoked_for_git_commit() {
        return Ok(0);
    }

    let config = Config::load(repo_root);
    let blocking_policy = config.block_at;
    let index = FingerprintIndex::open(&Config::index_path(repo_root)).ok();
    let mut engine = Engine::new(config);
    if let Some(index) = index {
        engine = engine.with_index(index);
    }

    let target = if args.worktree {
        DiffTarget::WorktreeVsHead
    } else {
        DiffTarget::StagedVsHead
    };

    // Authorship comes from the file watcher in the app; the CLI has only the
    // commit-trailer signal available, so tags stay conservative here.
    let authorship = AuthorshipMap::new();
    let report = engine.analyze_repo(repo_root, target, &authorship)?;

    render_report(&report, args)?;

    // Record the run so `dross history` shows a trend from CI and hook runs,
    // not only from the desktop app. Failing to record must not fail the
    // check itself — a read-only index is not a reason to block a commit.
    if !args.no_record
        && let Ok(index) = FingerprintIndex::open(&Config::index_path(repo_root))
        && let Err(e) = index.record_findings(None, &report.findings)
    {
        eprintln!("{} could not record history: {e}", "note:".yellow());
    }

    // An explicit flag wins so a CI job can be stricter than the repository's
    // own policy; otherwise the repository setting applies, and `error` is the
    // fallback when neither is set.
    let threshold = args
        .block_at
        .map(Severity::from)
        .or(blocking_policy)
        .unwrap_or(Severity::Error);

    if args.hook && report.has_blocking(threshold) {
        return Ok(1);
    }
    Ok(0)
}

fn render_report(report: &Report, args: &CheckArgs) -> Result<()> {
    match args.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(report)?),
        OutputFormat::Compact => render::compact(report),
        OutputFormat::Human => {
            if args.quiet {
                println!("{}", report.summary_line());
            } else {
                render::human(report);
            }
        }
    }
    Ok(())
}

fn cmd_index(args: &IndexArgs, repo_root: &Path) -> Result<i32> {
    let config = Config::load(repo_root);
    let index = FingerprintIndex::open(&Config::index_path(repo_root))?;
    let mut engine = Engine::new(config).with_index(index);

    println!("{} building fingerprint index…", "dross:".cyan().bold());
    let mut last_pct = 0;
    let indexed = engine.build_index(repo_root, |done, total| {
        let pct = (done * 100).checked_div(total).unwrap_or(100);
        if pct >= last_pct + 10 {
            last_pct = pct;
            println!("  {pct:>3}%  ({done}/{total} files)");
        }
    })?;
    println!("  indexed {indexed} functions");

    if args.baseline {
        println!(
            "{} replaying history for complexity baseline…",
            "dross:".cyan().bold()
        );
        let samples = engine.build_complexity_baseline(repo_root, args.baseline_commits)?;
        println!("  recorded {samples} baseline samples");
        if samples < 30 {
            println!(
                "  {} fewer than 30 samples — the complexity-outlier signal stays \
                 silent until the baseline is large enough to be meaningful",
                "note:".yellow().bold()
            );
        }
    }
    Ok(0)
}

fn cmd_connections(args: &ConnectionsArgs, repo_root: &Path) -> Result<i32> {
    use dross_adapters::{all_adapters, detect_all};

    match &args.action {
        None | Some(ConnectionAction::List) => {
            render::connections(&detect_all(repo_root));
            Ok(0)
        }
        Some(ConnectionAction::Install { tool }) => {
            let adapters = all_adapters();
            let adapter = adapters
                .iter()
                .find(|a| matches_tool(a.id(), tool))
                .with_context(|| format!("unknown tool `{tool}`"))?;
            adapter.install(repo_root)?;
            println!(
                "{} wired into {}",
                "dross:".green().bold(),
                adapter.id().label()
            );
            for limitation in adapter.status(repo_root).limitations {
                println!("  {} {limitation}", "note:".yellow().bold());
            }
            Ok(0)
        }
        Some(ConnectionAction::Uninstall { tool }) => {
            let adapters = all_adapters();
            let adapter = adapters
                .iter()
                .find(|a| matches_tool(a.id(), tool))
                .with_context(|| format!("unknown tool `{tool}`"))?;
            adapter.uninstall(repo_root)?;
            println!(
                "{} removed from {}",
                "dross:".green().bold(),
                adapter.id().label()
            );
            Ok(0)
        }
    }
}

fn matches_tool(id: dross_adapters::AdapterId, name: &str) -> bool {
    use dross_adapters::AdapterId::*;
    let n = name.to_ascii_lowercase().replace(['-', '_', ' '], "");
    match id {
        ClaudeCode => matches!(n.as_str(), "claude" | "claudecode"),
        CodexCli => matches!(n.as_str(), "codex" | "codexcli"),
        Antigravity => matches!(n.as_str(), "antigravity" | "gemini"),
        GitHook => matches!(n.as_str(), "git" | "githook" | "precommit"),
    }
}

fn cmd_history(args: &HistoryArgs, repo_root: &Path) -> Result<i32> {
    let index = FingerprintIndex::open(&Config::index_path(repo_root))?;
    let entries = index.risk_history(args.limit)?;
    match args.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&entries)?),
        _ => render::history(&entries),
    }
    Ok(0)
}

fn cmd_init(repo_root: &Path) -> Result<i32> {
    let config = Config::default();
    config.save(repo_root)?;
    println!(
        "{} wrote {}",
        "dross:".green().bold(),
        repo_root.join(".dross.json").display()
    );
    println!("  next: `dross index` to build the fingerprint index");
    println!("  then: `dross connections install git` for the pre-commit hook");
    Ok(0)
}

fn resolve_repo(explicit: Option<&Path>) -> Result<PathBuf> {
    let start = match explicit {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let repo = dross_core::diff::Repo::open(&start)?;
    Ok(repo.workdir()?.to_path_buf())
}

/// Detects whether the hook was triggered by a `git commit`. Agent Bash-tool
/// hooks fire on every command, so the command line is passed through an env
/// var by the adapter wrappers.
fn invoked_for_git_commit() -> bool {
    for key in [
        "DROSS_HOOK_COMMAND",
        "CLAUDE_TOOL_INPUT",
        "CODEX_TOOL_INPUT",
        "TOOL_INPUT",
    ] {
        if let Ok(value) = std::env::var(key) {
            let lower = value.to_ascii_lowercase();
            if lower.contains("git commit") || lower.contains("git ci") {
                return true;
            }
        }
    }
    false
}
