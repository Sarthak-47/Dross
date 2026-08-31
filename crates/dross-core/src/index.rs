//! SQLite-backed fingerprint index and risk-history log.
//!
//! Built once on first open (spec section 4), then updated incrementally. The
//! index is the repo's own control group — both the clone detector and the
//! over-engineering complexity baseline read from it.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::ast::ParsedFile;
use crate::fingerprint::{Fingerprint, NUM_HASHES, fingerprint};
use crate::lang::Language;

/// Bumped whenever fingerprinting or normalization changes, so a stale index
/// is rebuilt rather than silently compared against incompatible signatures.
const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFunction {
    pub id: i64,
    pub path: PathBuf,
    pub name: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub fingerprint: Fingerprint,
    pub line_count: usize,
    pub node_count: usize,
    pub cyclomatic: usize,
}

pub struct FingerprintIndex {
    conn: Connection,
}

impl FingerprintIndex {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)
            .with_context(|| format!("opening index at {}", db_path.display()))?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    pub fn open_in_memory() -> Result<Self> {
        let index = Self {
            conn: Connection::open_in_memory()?,
        };
        index.migrate()?;
        Ok(index)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS functions (
                id          INTEGER PRIMARY KEY,
                path        TEXT NOT NULL,
                name        TEXT,
                start_line  INTEGER NOT NULL,
                end_line    INTEGER NOT NULL,
                line_count  INTEGER NOT NULL,
                node_count  INTEGER NOT NULL,
                cyclomatic  INTEGER NOT NULL,
                signature   BLOB NOT NULL,
                shingles    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_functions_path ON functions(path);

            -- LSH bands: lets clone lookup skip a full table scan.
            CREATE TABLE IF NOT EXISTS bands (
                function_id INTEGER NOT NULL,
                band_index  INTEGER NOT NULL,
                band_hash   INTEGER NOT NULL,
                FOREIGN KEY(function_id) REFERENCES functions(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_bands_lookup ON bands(band_index, band_hash);

            -- Risk history for the trend dashboard (spec section 5 / week 10).
            CREATE TABLE IF NOT EXISTS risk_history (
                id           INTEGER PRIMARY KEY,
                recorded_at  TEXT NOT NULL,
                commit_sha   TEXT,
                check_id     TEXT NOT NULL,
                signal       TEXT NOT NULL,
                severity     TEXT NOT NULL,
                count        INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_risk_time ON risk_history(recorded_at);

            -- Cached per-file symbols. Without this the symbol table is
            -- rebuilt by re-parsing every file in the repository on every
            -- run, which made a single-file check take seconds on a large
            -- tree -- unusable for a pre-commit hook.
            CREATE TABLE IF NOT EXISTS file_symbols (
                path    TEXT PRIMARY KEY,
                symbols TEXT NOT NULL
            );

            -- Per-repo complexity baseline (spec 5a).
            CREATE TABLE IF NOT EXISTS complexity_baseline (
                id            INTEGER PRIMARY KEY,
                commit_sha    TEXT NOT NULL,
                lines_changed INTEGER NOT NULL,
                complexity    INTEGER NOT NULL,
                ratio         REAL NOT NULL
            );
            "#,
        )?;

        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;

        match existing {
            Some(v) if v == SCHEMA_VERSION.to_string() => {}
            Some(_) => {
                // Fingerprints from an older normalization are not comparable.
                self.conn.execute_batch(
                    "DELETE FROM bands; DELETE FROM functions; DELETE FROM complexity_baseline; DELETE FROM file_symbols;",
                )?;
                self.set_meta("schema_version", &SCHEMA_VERSION.to_string())?;
            }
            None => self.set_meta("schema_version", &SCHEMA_VERSION.to_string())?,
        }
        Ok(())
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn function_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM functions", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Replaces every indexed function for a file. Called on first index and
    /// whenever a file changes.
    pub fn index_file(&mut self, path: &Path, language: Language, source: &str) -> Result<usize> {
        let parsed = ParsedFile::parse(language, source)?;
        let path_str = path.to_string_lossy().to_string();
        let tx = self.conn.transaction()?;

        tx.execute(
            "DELETE FROM bands WHERE function_id IN (SELECT id FROM functions WHERE path = ?1)",
            params![path_str],
        )?;
        tx.execute("DELETE FROM functions WHERE path = ?1", params![path_str])?;

        // Cache this file's symbols alongside its fingerprints so the check
        // run does not have to re-parse the whole repository.
        let symbols = crate::symbols::FileSymbols::extract(&parsed);
        tx.execute(
            "INSERT INTO file_symbols(path, symbols) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET symbols = excluded.symbols",
            params![path_str, serde_json::to_string(&symbols)?],
        )?;

        let mut count = 0;
        for func in parsed.functions() {
            let fp = fingerprint(&parsed, &func);
            // Tiny functions produce degenerate fingerprints and would flood
            // the clone check with noise.
            if fp.shingle_count < 5 {
                continue;
            }
            let metrics = crate::metrics::function_metrics(&parsed, &func);
            tx.execute(
                "INSERT INTO functions
                 (path, name, start_line, end_line, line_count, node_count, cyclomatic, signature, shingles)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    path_str,
                    func.name,
                    func.start_line as i64,
                    func.end_line as i64,
                    func.line_count() as i64,
                    metrics.node_count as i64,
                    metrics.cyclomatic as i64,
                    encode_signature(&fp.signature),
                    fp.shingle_count as i64,
                ],
            )?;
            let id = tx.last_insert_rowid();
            for (band_index, band_hash) in band_hashes(&fp.signature) {
                tx.execute(
                    "INSERT INTO bands(function_id, band_index, band_hash) VALUES (?1, ?2, ?3)",
                    params![id, band_index as i64, { band_hash }],
                )?;
            }
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }

    pub fn remove_file(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();
        self.conn.execute(
            "DELETE FROM bands WHERE function_id IN (SELECT id FROM functions WHERE path = ?1)",
            params![path_str],
        )?;
        self.conn
            .execute("DELETE FROM functions WHERE path = ?1", params![path_str])?;
        self.conn.execute(
            "DELETE FROM file_symbols WHERE path = ?1",
            params![path_str],
        )?;
        Ok(())
    }

    /// Loads every cached file's symbols, skipping paths the caller will
    /// supply itself (the changed files, whose on-disk content may be stale
    /// relative to what is staged).
    pub fn load_symbols(
        &self,
        exclude: &std::collections::HashSet<PathBuf>,
    ) -> Result<Vec<(PathBuf, crate::symbols::FileSymbols)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, symbols FROM file_symbols")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;

        let mut out = Vec::new();
        for row in rows {
            let (path, json) = row?;
            let path = PathBuf::from(path);
            if exclude.contains(&path) {
                continue;
            }
            // A row that fails to parse is skipped rather than aborting the
            // run; a stale cache entry should degrade recall, not break the
            // check entirely.
            if let Ok(symbols) = serde_json::from_str(&json) {
                out.push((path, symbols));
            }
        }
        Ok(out)
    }

    pub fn symbol_file_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM file_symbols", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Candidate near-duplicates via LSH banding, then exact signature compare.
    pub fn find_similar(
        &self,
        fp: &Fingerprint,
        threshold: f64,
        exclude_path: Option<&Path>,
    ) -> Result<Vec<(IndexedFunction, f64)>> {
        let mut candidate_ids: Vec<i64> = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT function_id FROM bands WHERE band_index = ?1 AND band_hash = ?2",
            )?;
            for (band_index, band_hash) in band_hashes(&fp.signature) {
                let rows = stmt.query_map(params![band_index as i64, { band_hash }], |r| {
                    r.get::<_, i64>(0)
                })?;
                for id in rows {
                    candidate_ids.push(id?);
                }
            }
        }
        candidate_ids.sort_unstable();
        candidate_ids.dedup();

        let mut out = Vec::new();
        for id in candidate_ids {
            let Some(func) = self.load_function(id)? else {
                continue;
            };
            if let Some(exclude) = exclude_path
                && func.path == exclude
            {
                continue;
            }
            let sim = fp.similarity(&func.fingerprint);
            if sim >= threshold {
                out.push((func, sim));
            }
        }
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    fn load_function(&self, id: i64) -> Result<Option<IndexedFunction>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, path, name, start_line, end_line, line_count, node_count, cyclomatic, signature, shingles
                 FROM functions WHERE id = ?1",
                params![id],
                |r| {
                    let sig: Vec<u8> = r.get(8)?;
                    Ok(IndexedFunction {
                        id: r.get(0)?,
                        path: PathBuf::from(r.get::<_, String>(1)?),
                        name: r.get(2)?,
                        start_line: r.get::<_, i64>(3)? as usize,
                        end_line: r.get::<_, i64>(4)? as usize,
                        line_count: r.get::<_, i64>(5)? as usize,
                        node_count: r.get::<_, i64>(6)? as usize,
                        cyclomatic: r.get::<_, i64>(7)? as usize,
                        fingerprint: Fingerprint {
                            signature: decode_signature(&sig),
                            shingle_count: r.get::<_, i64>(9)? as usize,
                        },
                    })
                },
            )
            .optional()?)
    }

    // --- complexity baseline (spec 5a) ---

    pub fn record_baseline_sample(
        &self,
        commit_sha: &str,
        lines_changed: usize,
        complexity: usize,
    ) -> Result<()> {
        let ratio = complexity as f64 / (lines_changed.max(1)) as f64;
        self.conn.execute(
            "INSERT INTO complexity_baseline(commit_sha, lines_changed, complexity, ratio)
             VALUES (?1, ?2, ?3, ?4)",
            params![commit_sha, lines_changed as i64, complexity as i64, ratio],
        )?;
        Ok(())
    }

    /// Mean and standard deviation of complexity-per-line across the repo's
    /// own history — the control group the outlier signal scores against.
    pub fn baseline_stats(&self) -> Result<Option<BaselineStats>> {
        let ratios: Vec<f64> = {
            let mut stmt = self.conn.prepare("SELECT ratio FROM complexity_baseline")?;
            let rows = stmt.query_map([], |r| r.get::<_, f64>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        // Below ~30 samples a z-score is not meaningful; the check should stay
        // silent rather than fire on noise.
        if ratios.len() < 30 {
            return Ok(None);
        }
        let n = ratios.len() as f64;
        let mean = ratios.iter().sum::<f64>() / n;
        let variance = ratios.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
        Ok(Some(BaselineStats {
            mean,
            std_dev: variance.sqrt(),
            sample_count: ratios.len(),
        }))
    }

    // --- risk history (spec section 5 / week 10) ---

    pub fn record_findings(
        &self,
        commit_sha: Option<&str>,
        findings: &[crate::finding::Finding],
    ) -> Result<()> {
        use std::collections::HashMap;
        let now = chrono::Utc::now().to_rfc3339();
        let mut buckets: HashMap<(String, String, String), usize> = HashMap::new();
        for f in findings {
            let key = (
                f.check.as_str().to_string(),
                f.signal.clone(),
                format!("{:?}", f.severity).to_lowercase(),
            );
            *buckets.entry(key).or_insert(0) += 1;
        }
        for ((check, signal, severity), count) in buckets {
            self.conn.execute(
                "INSERT INTO risk_history(recorded_at, commit_sha, check_id, signal, severity, count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![now, commit_sha, check, signal, severity, count as i64],
            )?;
        }
        Ok(())
    }

    pub fn risk_history(&self, limit: usize) -> Result<Vec<RiskEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT recorded_at, commit_sha, check_id, signal, severity, count
             FROM risk_history ORDER BY recorded_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(RiskEntry {
                recorded_at: r.get(0)?,
                commit_sha: r.get(1)?,
                check_id: r.get(2)?,
                signal: r.get(3)?,
                severity: r.get(4)?,
                count: r.get::<_, i64>(5)? as usize,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BaselineStats {
    pub mean: f64,
    pub std_dev: f64,
    pub sample_count: usize,
}

impl BaselineStats {
    pub fn z_score(&self, ratio: f64) -> f64 {
        if self.std_dev <= f64::EPSILON {
            return 0.0;
        }
        (ratio - self.mean) / self.std_dev
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskEntry {
    pub recorded_at: String,
    pub commit_sha: Option<String>,
    pub check_id: String,
    pub signal: String,
    pub severity: String,
    pub count: usize,
}

/// LSH banding: 32 bands of 4 rows each over the 128-slot signature.
const BAND_ROWS: usize = 4;

fn band_hashes(signature: &[u64]) -> Vec<(usize, i64)> {
    if signature.len() != NUM_HASHES {
        return Vec::new();
    }
    signature
        .chunks(BAND_ROWS)
        .enumerate()
        .map(|(i, chunk)| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for v in chunk {
                h ^= *v;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
            (i, h as i64)
        })
        .collect()
}

fn encode_signature(sig: &[u64]) -> Vec<u8> {
    sig.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn decode_signature(bytes: &[u8]) -> Vec<u64> {
    // Written out rather than using `chunks_exact` or `as_chunks`: the former
    // is linted by newer clippy, and the latter's stabilisation is later than
    // this workspace's declared MSRV. This form needs neither and has no
    // fallible conversion to unwrap. A trailing partial chunk cannot occur for
    // a signature this module wrote, and is ignored rather than panicked on.
    const WIDTH: usize = size_of::<u64>();
    let mut out = Vec::with_capacity(bytes.len() / WIDTH);
    let mut offset = 0;
    while offset + WIDTH <= bytes.len() {
        let mut word = [0u8; WIDTH];
        word.copy_from_slice(&bytes[offset..offset + WIDTH]);
        out.push(u64::from_le_bytes(word));
        offset += WIDTH;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_and_finds_a_near_duplicate() {
        let mut index = FingerprintIndex::open_in_memory().unwrap();
        let existing = "export function computeTotal(items) {\n  let total = 0;\n  for (const item of items) {\n    total += item.price * item.quantity;\n  }\n  return total;\n}\n";
        index
            .index_file(Path::new("src/cart.js"), Language::JavaScript, existing)
            .unwrap();
        assert_eq!(index.function_count().unwrap(), 1);

        // Same logic, different names — the classic agent reinvention.
        let dup = "export function sumBasket(entries) {\n  let acc = 0;\n  for (const entry of entries) {\n    acc += entry.price * entry.quantity;\n  }\n  return acc;\n}\n";
        let parsed = ParsedFile::parse(Language::JavaScript, dup).unwrap();
        let func = parsed.functions().into_iter().next().unwrap();
        let fp = fingerprint(&parsed, &func);

        let hits = index
            .find_similar(&fp, 0.8, Some(Path::new("src/checkout.js")))
            .unwrap();
        assert!(!hits.is_empty(), "expected to find the near-duplicate");
        assert_eq!(hits[0].0.name.as_deref(), Some("computeTotal"));
    }

    #[test]
    fn excludes_the_functions_own_file() {
        let mut index = FingerprintIndex::open_in_memory().unwrap();
        let src = "function computeTotal(items) {\n  let total = 0;\n  for (const i of items) {\n    total += i.price * i.quantity;\n  }\n  return total;\n}\n";
        index
            .index_file(Path::new("src/cart.js"), Language::JavaScript, src)
            .unwrap();
        let parsed = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let func = parsed.functions().into_iter().next().unwrap();
        let fp = fingerprint(&parsed, &func);
        let hits = index
            .find_similar(&fp, 0.8, Some(Path::new("src/cart.js")))
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn baseline_stays_silent_below_thirty_samples() {
        let index = FingerprintIndex::open_in_memory().unwrap();
        for i in 0..10 {
            index.record_baseline_sample("abc", 10 + i, 20 + i).unwrap();
        }
        assert!(index.baseline_stats().unwrap().is_none());
    }

    #[test]
    fn baseline_reports_stats_once_warm() {
        let index = FingerprintIndex::open_in_memory().unwrap();
        for i in 0..40 {
            index
                .record_baseline_sample("abc", 10, 20 + (i % 3))
                .unwrap();
        }
        let stats = index.baseline_stats().unwrap().unwrap();
        assert_eq!(stats.sample_count, 40);
        assert!(stats.mean > 0.0);
    }

    /// Re-indexing a file has to replace what was there, not add to it.
    ///
    /// Untested until now, and the failure is silent: a function deleted from
    /// a file would stay in the index and go on matching, so clone detection
    /// would report a duplicate of code that no longer exists.
    #[test]
    fn reindexing_a_file_replaces_its_previous_functions() {
        let mut index = FingerprintIndex::open_in_memory().unwrap();
        let path = Path::new("src/a.js");
        let two = "export function one(a) {
  return a + 1;
}
export function two(b) {
  return b * 2;
}
";
        index.index_file(path, Language::JavaScript, two).unwrap();
        assert_eq!(index.function_count().unwrap(), 2);

        // `two` is deleted from the file.
        let one = "export function one(a) {
  return a + 1;
}
";
        index.index_file(path, Language::JavaScript, one).unwrap();
        assert_eq!(
            index.function_count().unwrap(),
            1,
            "the removed function survived a re-index"
        );

        // And its band rows went with it, or it would still be matchable.
        let parsed = ParsedFile::parse(
            Language::JavaScript,
            "function gone(b) {
  return b * 2;
}
",
        )
        .unwrap();
        let func = parsed.functions().into_iter().next().unwrap();
        let fp = fingerprint(&parsed, &func);
        let hits = index
            .find_similar(&fp, 0.8, Some(Path::new("src/other.js")))
            .unwrap();
        assert!(
            hits.is_empty(),
            "a deleted function was still matchable: {hits:?}"
        );
    }

    #[test]
    fn removing_a_file_clears_its_functions_and_symbols() {
        let mut index = FingerprintIndex::open_in_memory().unwrap();
        let path = Path::new("src/a.js");
        index
            .index_file(
                path,
                Language::JavaScript,
                "export function one(a) {
  return a + 1;
}
",
            )
            .unwrap();
        assert_eq!(index.function_count().unwrap(), 1);
        assert_eq!(index.symbol_file_count().unwrap(), 1);

        index.remove_file(path).unwrap();
        assert_eq!(index.function_count().unwrap(), 0);
        assert_eq!(
            index.symbol_file_count().unwrap(),
            0,
            "cached symbols outlived the file"
        );
    }

    /// A schema bump means fingerprints from the old normalization are not
    /// comparable with new ones. Leaving them in place would produce matches
    /// between two different encodings of the same code.
    #[test]
    fn a_schema_bump_discards_fingerprints_from_the_old_normalization() {
        let dir = std::env::temp_dir().join(format!("dross-idx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("index.sqlite");

        {
            let mut index = FingerprintIndex::open(&db).unwrap();
            index
                .index_file(
                    Path::new("src/a.js"),
                    Language::JavaScript,
                    "export function one(a) {
  return a + 1;
}
",
                )
                .unwrap();
            assert_eq!(index.function_count().unwrap(), 1);
            // Pretend this index was written by an earlier normalization.
            index.set_meta("schema_version", "0").unwrap();
        }

        let reopened = FingerprintIndex::open(&db).unwrap();
        assert_eq!(
            reopened.function_count().unwrap(),
            0,
            "fingerprints from an older schema were kept"
        );
        assert_eq!(
            reopened.get_meta("schema_version").unwrap().as_deref(),
            Some("1"),
            "the schema version was not brought forward"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reopening at the current version must keep the index, or every run
    /// would rebuild it from scratch and the cache would buy nothing.
    #[test]
    fn reopening_at_the_same_schema_keeps_the_index() {
        let dir = std::env::temp_dir().join(format!("dross-idx-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("index.sqlite");

        {
            let mut index = FingerprintIndex::open(&db).unwrap();
            index
                .index_file(
                    Path::new("src/a.js"),
                    Language::JavaScript,
                    "export function one(a) {
  return a + 1;
}
",
                )
                .unwrap();
        }
        let reopened = FingerprintIndex::open(&db).unwrap();
        assert_eq!(reopened.function_count().unwrap(), 1);
        assert_eq!(reopened.symbol_file_count().unwrap(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
