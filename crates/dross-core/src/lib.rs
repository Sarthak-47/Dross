//! Dross core — deterministic, model-free analysis of agent-generated diffs.
//!
//! Every check in this crate is a parser, a hash, or a graph algorithm. There
//! are no model calls anywhere in the pipeline, which is what makes a run
//! reproducible, offline, and free.

pub mod ast;
pub mod authorship;
pub mod checks;
pub mod config;
pub mod diff;
pub mod engine;
pub mod finding;
pub mod fingerprint;
pub mod index;
pub mod lang;
pub mod metrics;
pub mod symbols;

pub use config::Config;
pub use engine::{Engine, Report};
pub use finding::{CheckId, Finding, Severity};
pub use lang::Language;
