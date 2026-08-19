//! AI-authorship tagging (spec section 5).
//!
//! Two signals with very different reliability:
//!   * commit trailers / session markers — reliable when present,
//!   * burst-write timestamps — heuristic, and the one that carries real load.
//!
//! The asymmetry matters: a human hunk mistagged as AI just runs an extra
//! check, but an AI hunk mistagged as human silently drops to the lighter
//! pass. So confidence is a first-class value that reaches the UI rather than
//! being collapsed into a bool.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tag {
    /// A trailer or session marker explicitly named an agent.
    Confirmed,
    /// Burst-write timing suggested an agent wrote this.
    Heuristic,
    /// No agent signal.
    Human,
    /// The user re-tagged this range in the UI; bool = is_ai.
    UserOverride(bool),
}

impl Tag {
    pub fn is_ai(&self) -> bool {
        match self {
            Tag::Confirmed | Tag::Heuristic => true,
            Tag::Human => false,
            Tag::UserOverride(v) => *v,
        }
    }

    /// Confirmed beats heuristic beats human; a user override beats everything.
    fn rank(&self) -> u8 {
        match self {
            Tag::UserOverride(_) => 3,
            Tag::Confirmed => 2,
            Tag::Heuristic => 1,
            Tag::Human => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggedRange {
    pub start_line: usize,
    pub end_line: usize,
    pub tag: Tag,
    /// What produced this tag, shown in the UI so the user can judge it.
    pub reason: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AuthorshipMap {
    ranges: HashMap<PathBuf, Vec<TaggedRange>>,
}

impl AuthorshipMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<PathBuf>, range: TaggedRange) {
        self.ranges.entry(path.into()).or_default().push(range);
    }

    /// Highest-ranked tag covering this line, defaulting to `Human`.
    pub fn tag_for(&self, path: &Path, line: usize) -> Tag {
        self.ranges
            .get(path)
            .into_iter()
            .flatten()
            .filter(|r| line >= r.start_line && line <= r.end_line)
            .max_by_key(|r| r.tag.rank())
            .map(|r| r.tag.clone())
            .unwrap_or(Tag::Human)
    }

    pub fn ranges_for(&self, path: &Path) -> &[TaggedRange] {
        self.ranges.get(path).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Every tagged path and its ranges, for merging two maps.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &Vec<TaggedRange>)> {
        self.ranges.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// User re-tags a range from the UI. Recorded as a separate range so the
    /// original detection stays inspectable.
    pub fn override_range(
        &mut self,
        path: impl Into<PathBuf>,
        start_line: usize,
        end_line: usize,
        is_ai: bool,
    ) {
        self.insert(
            path,
            TaggedRange {
                start_line,
                end_line,
                tag: Tag::UserOverride(is_ai),
                reason: "manually re-tagged by user".to_string(),
            },
        );
    }
}

/// Commit trailers that name a coding agent as (co-)author.
const AGENT_TRAILER_NEEDLES: [&str; 6] = [
    "claude",
    "codex",
    "antigravity",
    "cursor",
    "copilot",
    "gemini",
];

/// Scans a commit message for agent trailers.
///
/// Note this only fires when the tool actually writes a trailer, which not
/// every agent does by default — hence the burst-write fallback.
pub fn tag_from_commit_message(message: &str) -> Option<(Tag, String)> {
    for line in message.lines() {
        let lower = line.trim().to_ascii_lowercase();
        let is_trailer = lower.starts_with("co-authored-by:")
            || lower.starts_with("generated-with:")
            || lower.starts_with("assisted-by:");
        if !is_trailer {
            continue;
        }
        if let Some(agent) = AGENT_TRAILER_NEEDLES.iter().find(|n| lower.contains(*n)) {
            return Some((Tag::Confirmed, format!("commit trailer names `{agent}`")));
        }
    }
    None
}

/// Parameters for the burst-write heuristic.
#[derive(Debug, Clone, Copy)]
pub struct BurstParams {
    /// Max gap between consecutive writes for them to count as one burst.
    pub max_gap_ms: u64,
    /// Minimum writes in a burst before it looks agent-driven.
    pub min_writes: usize,
    /// Minimum lines changed across the burst.
    pub min_lines: usize,
}

impl Default for BurstParams {
    fn default() -> Self {
        // Deliberately conservative: a burst must be both fast and large to
        // clear the bar, because a false "human" tag is the costly direction
        // and a false "AI" tag only costs an extra check.
        Self {
            max_gap_ms: 2_000,
            min_writes: 3,
            min_lines: 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WriteEvent {
    pub path: PathBuf,
    pub timestamp_ms: u64,
    pub start_line: usize,
    pub end_line: usize,
}

/// Groups file-watcher write events into bursts and tags large fast bursts.
pub fn tag_from_write_bursts(events: &[WriteEvent], params: BurstParams) -> AuthorshipMap {
    let mut map = AuthorshipMap::new();
    if events.is_empty() {
        return map;
    }

    let mut sorted = events.to_vec();
    sorted.sort_by_key(|e| e.timestamp_ms);

    let mut burst: Vec<&WriteEvent> = vec![&sorted[0]];
    let flush = |burst: &Vec<&WriteEvent>, map: &mut AuthorshipMap| {
        let lines: usize = burst
            .iter()
            .map(|e| e.end_line.saturating_sub(e.start_line) + 1)
            .sum();
        if burst.len() < params.min_writes || lines < params.min_lines {
            return;
        }
        let span_ms = burst
            .last()
            .map(|l| l.timestamp_ms)
            .unwrap_or(0)
            .saturating_sub(burst[0].timestamp_ms);
        for event in burst {
            map.insert(
                event.path.clone(),
                TaggedRange {
                    start_line: event.start_line,
                    end_line: event.end_line,
                    tag: Tag::Heuristic,
                    reason: format!(
                        "{} writes totalling {lines} lines within {span_ms}ms",
                        burst.len()
                    ),
                },
            );
        }
    };

    for event in sorted.iter().skip(1) {
        let prev = burst.last().unwrap();
        if event.timestamp_ms.saturating_sub(prev.timestamp_ms) <= params.max_gap_ms {
            burst.push(event);
        } else {
            flush(&burst, &mut map);
            burst = vec![event];
        }
    }
    flush(&burst, &mut map);

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirms_from_agent_trailer() {
        let (tag, _) =
            tag_from_commit_message("feat: x\n\nCo-Authored-By: Claude <noreply@anthropic.com>")
                .unwrap();
        assert_eq!(tag, Tag::Confirmed);
    }

    #[test]
    fn ignores_human_coauthor_trailer() {
        assert!(tag_from_commit_message("feat: x\n\nCo-Authored-By: Alex <a@b.c>").is_none());
    }

    #[test]
    fn user_override_beats_detection() {
        let mut map = AuthorshipMap::new();
        map.insert(
            "a.ts",
            TaggedRange {
                start_line: 1,
                end_line: 10,
                tag: Tag::Heuristic,
                reason: "burst".into(),
            },
        );
        map.override_range("a.ts", 1, 10, false);
        assert_eq!(map.tag_for(Path::new("a.ts"), 5), Tag::UserOverride(false));
        assert!(!map.tag_for(Path::new("a.ts"), 5).is_ai());
    }

    #[test]
    fn large_fast_burst_is_tagged() {
        let events: Vec<WriteEvent> = (0..4)
            .map(|i| WriteEvent {
                path: PathBuf::from("a.ts"),
                timestamp_ms: 1000 + i * 300,
                start_line: 1 + (i as usize) * 10,
                end_line: 10 + (i as usize) * 10,
            })
            .collect();
        let map = tag_from_write_bursts(&events, BurstParams::default());
        assert_eq!(map.tag_for(Path::new("a.ts"), 5), Tag::Heuristic);
    }

    #[test]
    fn slow_typing_is_not_tagged() {
        let events: Vec<WriteEvent> = (0..4)
            .map(|i| WriteEvent {
                path: PathBuf::from("a.ts"),
                timestamp_ms: 1000 + i * 30_000,
                start_line: 1,
                end_line: 2,
            })
            .collect();
        let map = tag_from_write_bursts(&events, BurstParams::default());
        assert_eq!(map.tag_for(Path::new("a.ts"), 1), Tag::Human);
    }
}
