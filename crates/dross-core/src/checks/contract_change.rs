//! Silent API-contract-change detector (spec section 5).
//!
//! Diffs function signatures independent of body changes. The point is to
//! catch breakage that isn't visible in the diff itself: the changed line is
//! in one file, the breakage is in every caller.

use std::collections::HashMap;

use crate::ast::{FunctionDef, Param};
use crate::finding::{CheckId, Finding, Severity, SourceSpan};

use super::{Check, CheckContext};

pub struct ContractChangeCheck;

impl Check for ContractChangeCheck {
    fn id(&self) -> CheckId {
        CheckId::ContractChange
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();

        for file in ctx.diffs {
            let (Some(new_parsed), Some(old_parsed)) =
                (ctx.parsed(&file.path), ctx.parsed_old(file))
            else {
                continue;
            };

            let old_funcs: HashMap<String, FunctionDef> = old_parsed
                .functions()
                .into_iter()
                .filter_map(|f| f.qualified_name().map(|n| (n, f)))
                .collect();

            for new_func in new_parsed.functions() {
                let Some(name) = new_func.qualified_name() else {
                    continue;
                };
                let Some(old_func) = old_funcs.get(&name) else {
                    continue;
                };
                findings.extend(compare(&file.path, &name, old_func, &new_func));
            }
        }

        findings
    }
}

fn compare(
    path: &std::path::Path,
    name: &str,
    old: &FunctionDef,
    new: &FunctionDef,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let span = SourceSpan {
        file: path.to_path_buf(),
        start_line: new.start_line,
        end_line: new.start_line,
    };

    let required = |ps: &[Param]| ps.iter().filter(|p| !p.optional && !p.variadic).count();
    let old_required = required(&old.params);
    let new_required = required(&new.params);

    // Narrowing: callers that used to compile now don't.
    if new_required > old_required {
        let added: Vec<&str> = new
            .params
            .iter()
            .skip(old.params.len())
            .filter(|p| !p.optional && !p.variadic)
            .map(|p| p.name.as_str())
            .collect();
        findings.push(Finding::new(
            CheckId::ContractChange,
            "required-parameter-added",
            Severity::Error,
            span.clone(),
            format!("`{name}` now requires {new_required} arguments, was {old_required}"),
            if added.is_empty() {
                "An existing optional parameter became required. Every existing call site \
                 that omitted it now breaks."
                    .to_string()
            } else {
                format!(
                    "New required parameter(s): {}. Every existing call site breaks unless \
                     updated in the same change.",
                    added.join(", ")
                )
            },
        ));
    }

    // Removal: fewer parameters accepted than before.
    if new.params.len() < old.params.len() {
        let removed: Vec<&str> = old
            .params
            .iter()
            .skip(new.params.len())
            .map(|p| p.name.as_str())
            .collect();
        findings.push(Finding::new(
            CheckId::ContractChange,
            "parameter-removed",
            Severity::Warning,
            span.clone(),
            format!("`{name}` dropped {} parameter(s)", removed.len()),
            format!(
                "Removed: {}. Call sites still passing these will silently pass ignored \
                 arguments in JS, or fail outright in typed code.",
                removed.join(", ")
            ),
        ));
    }

    // Type changes on positionally-matched parameters.
    for (i, (o, n)) in old.params.iter().zip(&new.params).enumerate() {
        match (&o.ty, &n.ty) {
            (Some(ot), Some(nt)) if ot != nt => {
                findings.push(Finding::new(
                    CheckId::ContractChange,
                    "parameter-type-changed",
                    Severity::Warning,
                    span.clone(),
                    format!(
                        "`{name}` parameter #{} changed type: `{ot}` -> `{nt}`",
                        i + 1
                    ),
                    format!(
                        "Parameter `{}` was `{ot}` and is now `{nt}`. This is invisible at \
                         call sites until they are type-checked.",
                        n.name
                    ),
                ));
            }
            (Some(ot), None) => {
                findings.push(Finding::new(
                    CheckId::ContractChange,
                    "parameter-type-removed",
                    Severity::Info,
                    span.clone(),
                    format!("`{name}` parameter `{}` lost its type annotation", n.name),
                    format!("Was `{ot}`, now untyped — the contract is no longer enforced."),
                ));
            }
            _ => {}
        }
        if o.optional && !n.optional {
            findings.push(Finding::new(
                CheckId::ContractChange,
                "optional-parameter-became-required",
                Severity::Error,
                span.clone(),
                format!("`{name}` parameter `{}` is no longer optional", n.name),
                "Existing call sites that omitted this argument now break.".to_string(),
            ));
        }
    }

    // Return type change.
    if old.return_type != new.return_type
        && let (Some(ot), Some(nt)) = (&old.return_type, &new.return_type)
    {
        findings.push(Finding::new(
            CheckId::ContractChange,
            "return-type-changed",
            Severity::Warning,
            span.clone(),
            format!("`{name}` return type changed: `{ot}` -> `{nt}`"),
            format!(
                "Callers written against `{ot}` may mishandle `{nt}` without any \
                     diff-visible error at the call site."
            ),
        ));
    }

    // Sync -> async is a silent breaking change for every non-awaiting caller.
    if !old.is_async && new.is_async {
        findings.push(Finding::new(
            CheckId::ContractChange,
            "became-async",
            Severity::Error,
            span,
            format!("`{name}` became async"),
            "Callers that do not await now receive a Promise/coroutine instead of a value. \
             In JS this fails silently at runtime rather than at the call site."
                .to_string(),
        ));
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ParsedFile;
    use crate::lang::Language;

    fn compare_src(language: Language, old_src: &str, new_src: &str) -> Vec<Finding> {
        let old_file = ParsedFile::parse(language, old_src).unwrap();
        let new_file = ParsedFile::parse(language, new_src).unwrap();
        let old = old_file.functions().into_iter().next().unwrap();
        let new = new_file.functions().into_iter().next().unwrap();
        let name = new.qualified_name().unwrap();
        compare(std::path::Path::new("a.ts"), &name, &old, &new)
    }

    fn signals(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.signal.as_str()).collect()
    }

    #[test]
    fn flags_new_required_parameter() {
        let f = compare_src(
            Language::TypeScript,
            "function send(url: string) { return 1; }",
            "function send(url: string, retries: number) { return 1; }",
        );
        assert!(signals(&f).contains(&"required-parameter-added"));
    }

    #[test]
    fn ignores_new_optional_parameter() {
        let f = compare_src(
            Language::TypeScript,
            "function send(url: string) { return 1; }",
            "function send(url: string, retries?: number) { return 1; }",
        );
        assert!(f.is_empty(), "got {:?}", signals(&f));
    }

    #[test]
    fn flags_return_type_change() {
        let f = compare_src(
            Language::TypeScript,
            "function get(id: string): User { return u; }",
            "function get(id: string): User | null { return u; }",
        );
        assert!(signals(&f).contains(&"return-type-changed"));
    }

    #[test]
    fn flags_sync_to_async() {
        let f = compare_src(
            Language::TypeScript,
            "function load(id: string): User { return u; }",
            "async function load(id: string): User { return u; }",
        );
        assert!(signals(&f).contains(&"became-async"));
    }

    #[test]
    fn flags_optional_becoming_required() {
        let f = compare_src(
            Language::TypeScript,
            "function f(a: string, b?: number) { return 1; }",
            "function f(a: string, b: number) { return 1; }",
        );
        assert!(signals(&f).contains(&"optional-parameter-became-required"));
    }

    #[test]
    fn body_only_change_is_silent() {
        let f = compare_src(
            Language::TypeScript,
            "function f(a: string): number { return 1; }",
            "function f(a: string): number { return 2; }",
        );
        assert!(f.is_empty(), "got {:?}", signals(&f));
    }
}
