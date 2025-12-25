use tree_sitter::Parser;

fn main() {
    let code = r#"<?php

namespace App\Model;

use App\Enum\UserStatus;

class User
{
    #[Assert\Choice([UserStatus::ACTIVE, UserStatus::INACTIVE])]
    public string $status;
}
"#;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .unwrap();
    let tree = parser.parse(code, None).unwrap();
    let root = tree.root_node();

    print_tree(&root, code, 0);
}

fn print_tree(node: &tree_sitter::Node, source: &str, indent: usize) {
    let indent_str = "  ".repeat(indent);
    let text = node.utf8_text(source.as_bytes()).unwrap_or("");
    let text_preview = if text.len() > 50 {
        format!("{}...", &text[..50])
    } else {
        text.to_string()
    };

    println!(
        "{}{} [{}]: {:?}",
        indent_str,
        node.kind(),
        node.start_position().row,
        text_preview.replace('\n', "\\n")
    );

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_tree(&child, source, indent + 1);
    }
}
