use tree_sitter::{Node, Parser};
use tree_sitter_php::LANGUAGE_PHP;

fn print_tree(node: Node, source: &str, depth: usize) {
    let indent = "  ".repeat(depth);
    let text = node.utf8_text(source.as_bytes()).unwrap_or("");
    let text_preview = if text.len() > 50 {
        format!("{}...", &text[..50])
    } else {
        text.to_string()
    };

    println!(
        "{}{} {:?}",
        indent,
        node.kind(),
        text_preview.replace('\n', "\\n")
    );

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_tree(child, source, depth + 1);
    }
}

fn main() {
    let code = r#"<?php
namespace App\Entity;

use App\Base\BaseEntity;

class User extends BaseEntity {
}
"#;

    let mut parser = Parser::new();
    parser.set_language(&LANGUAGE_PHP.into()).unwrap();
    let tree = parser.parse(code, None).unwrap();

    println!("Tree structure for extends:");
    print_tree(tree.root_node(), code, 0);
}
