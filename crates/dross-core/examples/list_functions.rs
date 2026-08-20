//! Debug helper: lists the functions a file parses into, with their spans.
//! `cargo run -p dross-core --example list_functions -- python path/to/file.py`

use dross_core::ast::ParsedFile;
use dross_core::lang::Language;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let lang = args.next().unwrap_or_else(|| "python".into());
    let path = args.next().expect("usage: list_functions <lang> <file>");
    let filter = args.next();

    let language = match lang.as_str() {
        "javascript" | "js" => Language::JavaScript,
        "tsx" => Language::Tsx,
        "typescript" | "ts" => Language::TypeScript,
        _ => Language::Python,
    };

    let source = std::fs::read_to_string(&path)?;
    let file = ParsedFile::parse(language, source)?;
    println!("root has_error={}", file.root().has_error());

    for f in file.functions() {
        let name = f.qualified_name().unwrap_or_else(|| "<anon>".into());
        if let Some(needle) = &filter
            && !name.contains(needle.as_str())
        {
            continue;
        }
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| format!("{}:{}", p.name, p.ty.clone().unwrap_or_else(|| "-".into())))
            .collect();
        println!(
            "{:>6}-{:<6} {:<45} [{}]",
            f.start_line,
            f.end_line,
            name,
            params.join(", ")
        );
    }
    Ok(())
}
