#[cfg(test)]
mod debug_tests {
    use tree_sitter::Parser;
    use tree_sitter_php::LANGUAGE_PHP;

    #[test]
    fn debug_tree_structure() {
        let mut parser = Parser::new();
        parser.set_language(&LANGUAGE_PHP.into()).unwrap();
        let code = "<?php use A;";
        let tree = parser.parse(code, None).unwrap();
        println!("{}", tree.root_node().to_sexp());
    }
}
