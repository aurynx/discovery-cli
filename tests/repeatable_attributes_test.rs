use aurynx::metadata::AttributeArgument;
use aurynx::scanner::scan_directory;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_repeatable_attributes() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    let file_path = root.join("Repeatable.php");
    let mut f = File::create(&file_path).unwrap();
    writeln!(
        f,
        "<?php
namespace App;

#[Route('/a')]
#[Route('/b')]
class Repeatable {{}}
"
    )
    .unwrap();

    let paths = vec![root.to_path_buf()];
    let ignored = vec![];

    let results = scan_directory(&paths, &ignored);

    assert_eq!(results.len(), 1);
    let metadata = &results[0];
    assert_eq!(metadata.fqcn, "\\App\\Repeatable");

    let route_attrs = metadata
        .attributes
        .get("\\App\\Route")
        .expect("Route attributes missing");
    assert_eq!(route_attrs.len(), 2, "Should have 2 Route attributes");

    // Check first attribute
    let args1 = &route_attrs[0];
    assert_eq!(args1.len(), 1);
    match &args1[0] {
        AttributeArgument::Positional(val) => assert_eq!(val, "'/a'"),
        _ => panic!("Expected positional argument"),
    }

    // Check second attribute
    let args2 = &route_attrs[1];
    assert_eq!(args2.len(), 1);
    match &args2[0] {
        AttributeArgument::Positional(val) => assert_eq!(val, "'/b'"),
        _ => panic!("Expected positional argument"),
    }
}
