use aurynx::metadata::{
    AttributeArgument, ClassModifiers, MethodModifiers, PhpClassMetadata, PhpMethodMetadata,
};
use aurynx::writer::write_php_cache;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_compact_output_format() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("cache.php");

    let mut attributes = HashMap::new();
    attributes.insert(
        "\\App\\Attribute\\Route".to_string(),
        vec![vec![AttributeArgument::Positional("/api".to_string())]],
    );

    let metadata = PhpClassMetadata {
        fqcn: "\\App\\Test".to_string(),
        file: PathBuf::from("/tmp/test.php"),
        kind: "class".to_string(),
        modifiers: ClassModifiers::default(),
        attributes,
        extends: None,
        implements: vec![],
        methods: vec![PhpMethodMetadata {
            name: "index".to_string(),
            visibility: "public".to_string(),
            modifiers: MethodModifiers::default(),
            attributes: HashMap::new(),
            parameters: vec![],
            return_type: Some("void".to_string()),
        }],
        properties: vec![],
        backing_type: None,
        cases: vec![],
    };

    write_php_cache(&[metadata], &output_path, false).unwrap();

    let content = fs::read_to_string(&output_path).unwrap();

    // Check header
    assert!(
        content.starts_with("<?php declare(strict_types=1);"),
        "Header should be correct, got: {}",
        &content[..30.min(content.len())]
    );

    // Check for no trailing commas in arrays
    // A trailing comma would look like ",]" in compact mode
    assert!(
        !content.contains(",]"),
        "Output should not contain trailing commas in compact mode. Content snippet: {}",
        &content[..100.min(content.len())]
    );

    // Check specific structure to ensure valid PHP
    assert!(
        content.contains("'methods'=>['index'=>["),
        "Should contain method definition"
    );

    // Check that attributes don't have trailing comma
    // attributes=>['\\App\\Attribute\\Route'=>['/api']]
    // Note: In Rust string literals, \\ is a single backslash.
    // The file content should have double backslashes for namespaces (PHP string escaping).
    // So we need to look for \\\\App\\\\Attribute...
    assert!(
        content.contains("'attributes'=>['\\\\App\\\\Attribute\\\\Route'=>[['/api']]]"),
        "Attributes should be formatted correctly without trailing comma. Content: {}",
        content
    );
}
