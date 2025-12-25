use std::env;
use std::fs;
use tree_sitter::{Node, Parser};
use tree_sitter_php::LANGUAGE_PHP;

fn print_tree(node: Node, source: &str, depth: usize) {
    let indent = "  ".repeat(depth);
    let text = node.utf8_text(source.as_bytes()).unwrap_or("");
    let text_preview = if text.len() > 60 {
        format!("{}...", &text[..60].replace('\n', "\\n"))
    } else {
        text.replace('\n', "\\n")
    };

    println!("{}{} \"{}\"", indent, node.kind(), text_preview);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_tree(child, source, depth + 1);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let file_path = if args.len() > 1 {
        &args[1]
    } else {
        println!("Usage: cargo run --example debug_test_cases <test_case_number>");
        println!("Example: cargo run --example debug_test_cases 02");
        println!("\nAvailable test cases:");
        println!("  01 - Simple method");
        println!("  02 - Method with attribute");
        println!("  03 - Method with parameters");
        println!("  04 - Method with parameter attribute");
        println!("  05 - Static, abstract, final methods");
        println!("  06 - Multiple attributes");
        return;
    };

    // Build full path
    let test_file = if file_path.starts_with("0") {
        format!("../examples/test_cases/{}_*.php", file_path)
    } else if file_path.ends_with(".php") {
        file_path.to_string()
    } else {
        format!("../examples/test_cases/0{}_*.php", file_path)
    };

    // Find matching file
    let pattern = test_file.clone();
    let matches: Vec<_> = glob::glob(&pattern)
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    let actual_file = if !matches.is_empty() {
        matches[0].to_string_lossy().to_string()
    } else {
        // Try direct path
        if std::path::Path::new(file_path).exists() {
            file_path.to_string()
        } else {
            eprintln!("File not found: {}", test_file);
            return;
        }
    };

    println!("=== Parsing: {} ===\n", actual_file);

    let code = match fs::read_to_string(&actual_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return;
        }
    };

    let mut parser = Parser::new();
    parser
        .set_language(&LANGUAGE_PHP.into())
        .expect("Error loading PHP grammar");

    let tree = parser.parse(&code, None).expect("Error parsing code");

    println!("Full AST:");
    print_tree(tree.root_node(), &code, 0);
}
