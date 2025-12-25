use tree_sitter::{Node, Parser};
use tree_sitter_php::LANGUAGE_PHP;

fn print_tree(node: Node, source: &str, indent: usize) {
    let kind = node.kind();
    let text = if node.child_count() == 0 {
        format!(" => '{}'", &source[node.start_byte()..node.end_byte()])
    } else {
        String::new()
    };

    println!("{}{}{}", "  ".repeat(indent), kind, text);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_tree(child, source, indent + 1);
    }
}

fn main() {
    let code = "<?php
namespace App\\Enum;

enum Color: string
{
    case RED = 'red';

    public function label(): string
    {
        return 'test';
    }
}
";

    let mut parser = Parser::new();
    parser.set_language(&LANGUAGE_PHP.into()).unwrap();

    let tree = parser.parse(code, None).unwrap();
    let root = tree.root_node();

    println!("AST Tree:");
    print_tree(root, code, 0);
}
