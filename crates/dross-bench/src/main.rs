//! Benchmark harness (spec section 7) — the trust mechanism.
//!
//! Publishing precision/recall is the whole reason anyone installs a tool
//! that claims to catch things. Three constraints shape this harness:
//!
//! 1. Findings must be attributable to a commit, so a labeler can judge them
//!    against the change that produced them.
//! 2. The sample must distinguish agent-authored commits from human ones.
//!    Numbers computed over arbitrary commits do not demonstrate what the
//!    tool claims to detect.
//! 3. Labels must support two independent labelers, so the published figure
//!    can carry an inter-rater agreement score rather than one person's
//!    unchecked judgment.

mod label;
mod report;
mod run;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dross-bench",
    about = "Replays repository history through Dross and scores the results."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Replay commit history across one or more repositories.
    Run(run::RunArgs),
    /// Emit a labeling worksheet from a run's findings.
    Label(label::LabelArgs),
    /// Score labeled findings into precision/recall per check and signal.
    Report(report::ReportArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run::execute(args),
        Command::Label(args) => label::execute(args),
        Command::Report(args) => report::execute(args),
    }
}

/// Default location for benchmark data. Kept inside the project so a run
/// never writes outside the working tree.
pub fn default_workdir() -> PathBuf {
    PathBuf::from(".bench")
}
