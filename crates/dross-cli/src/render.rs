//! Terminal rendering. Findings must be scannable at a glance — a
//! pre-commit tool that buries the finding gets uninstalled.

use colored::{Color, Colorize};

use dross_adapters::AdapterStatus;
use dross_core::engine::Report;
use dross_core::finding::{AuthorshipConfidence, Finding, Severity};
use dross_core::index::RiskEntry;

pub fn human(report: &Report) {
    if report.findings.is_empty() {
        println!(
            "{} no findings in {} file(s) ({}ms)",
            "clean".green().bold(),
            report.files_analyzed,
            report.duration_ms
        );
        print_skipped(report);
        return;
    }

    println!();
    for finding in &report.findings {
        print_finding(finding);
    }

    println!(
        "{}  {} finding(s) — {} error, {} warning, {} info   risk {}/100   {}ms",
        "summary".bold(),
        report.findings.len(),
        count(report, Severity::Error).to_string().red(),
        count(report, Severity::Warning).to_string().yellow(),
        count(report, Severity::Info).to_string().cyan(),
        report.risk_score,
        report.duration_ms
    );
    print_skipped(report);
}

fn print_finding(finding: &Finding) {
    let (label, color) = match finding.severity {
        Severity::Error => ("error", Color::Red),
        Severity::Warning => ("warn", Color::Yellow),
        Severity::Info => ("info", Color::Cyan),
    };

    println!(
        "{} {}",
        format!("{label:>5}").color(color).bold(),
        finding.message.bold()
    );
    println!(
        "      {}:{}  {}",
        finding.span.file.display().to_string().dimmed(),
        finding.span.start_line.to_string().dimmed(),
        format!("[{}/{}]", finding.check.as_str(), finding.signal).dimmed()
    );
    println!("      {}", finding.evidence.dimmed());

    for related in &finding.related {
        println!(
            "      {} {}:{}",
            "see also".dimmed(),
            related.file.display().to_string().dimmed(),
            related.start_line.to_string().dimmed()
        );
    }

    // Authorship confidence is shown, never hidden — a heuristic tag the user
    // can see is a correctable one.
    if let Some(note) = authorship_note(finding.authorship) {
        println!("      {}", note.dimmed());
    }
    println!();
}

fn authorship_note(confidence: AuthorshipConfidence) -> Option<String> {
    match confidence {
        AuthorshipConfidence::Confirmed => {
            Some("authorship: agent-written (confirmed by trailer)".to_string())
        }
        AuthorshipConfidence::Heuristic => {
            Some("authorship: agent-written (heuristic — burst-write timing)".to_string())
        }
        AuthorshipConfidence::UserOverride => {
            Some("authorship: manually tagged".to_string())
        }
        AuthorshipConfidence::Unknown => None,
    }
}

fn print_skipped(report: &Report) {
    for skipped in &report.skipped {
        println!(
            "  {} {} skipped — {}",
            "note:".yellow().bold(),
            skipped.check,
            skipped.reason
        );
    }
}

fn count(report: &Report, severity: Severity) -> usize {
    report.count_by_severity(severity)
}

pub fn compact(report: &Report) {
    for f in &report.findings {
        println!(
            "{}:{}: {}: {} [{}/{}]",
            f.span.file.display(),
            f.span.start_line,
            match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            },
            f.message,
            f.check.as_str(),
            f.signal
        );
    }
}

pub fn connections(statuses: &[AdapterStatus]) {
    println!();
    for status in statuses {
        let state = if status.installed {
            "connected".green().bold()
        } else if status.detected {
            "detected".yellow().bold()
        } else {
            "not found".dimmed().bold()
        };
        println!("  {state:>12}  {}", status.label);
        if let Some(path) = &status.config_path {
            println!("                {}", path.display().to_string().dimmed());
        }
        for limitation in &status.limitations {
            println!("                {} {}", "note:".yellow(), limitation.dimmed());
        }
        println!();
    }
    println!(
        "  {}\n",
        "install with: dross connections install <claude|codex|antigravity|git>".dimmed()
    );
}

pub fn history(entries: &[RiskEntry]) {
    if entries.is_empty() {
        println!("no history recorded yet — run `dross check` to start the trend");
        return;
    }
    println!();
    println!(
        "  {:<26} {:<22} {:<10} {}",
        "when".bold(),
        "signal".bold(),
        "severity".bold(),
        "count".bold()
    );
    for e in entries {
        println!(
            "  {:<26} {:<22} {:<10} {}",
            e.recorded_at, e.signal, e.severity, e.count
        );
    }
    println!();
}
