//! Debug helper: prints the tree-sitter parse tree for a snippet.
//! `cargo run -p dross-core --example dump_ast -- typescript "class A {}"`

use dross_core::ast::ParsedFile;
use dross_core::lang::Language;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let lang = args.next().unwrap_or_else(|| "typescript".into());
    let src = args.next().unwrap_or_else(|| "class A {}".into());
    let language = match lang.as_str() {
        "javascript" | "js" => Language::JavaScript,
        "tsx" => Language::Tsx,
        "python" | "py" => Language::Python,
        _ => Language::TypeScript,
    };

    let file = ParsedFile::parse(language, src)?;
    print_node(&file, file.root(), 0);
    Ok(())
}

fn print_node(file: &ParsedFile, node: tree_sitter::Node<'_>, depth: usize) {
    let indent = "  ".repeat(depth);
    let text = file.text(node).replace('\n', "\\n");
    let text = if text.len() > 40 { &text[..40] } else { &text };
    println!("{indent}{} :: {text}", node.kind());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_node(file, child, depth + 1);
    }
}
