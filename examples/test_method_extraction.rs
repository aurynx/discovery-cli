use aurynx::parser::PhpMetadataExtractor;
use std::path::PathBuf;

fn main() {
    let code = r#"<?php

namespace Test;

use App\Attribute\Route;

class MethodWithAttribute
{
    #[Route('/test')]
    public function testMethod(): void
    {
    }
}
"#;

    let mut extractor = PhpMetadataExtractor::new().expect("Failed to create extractor");
    let metadata = extractor
        .extract_metadata(code, PathBuf::from("test.php"))
        .expect("Failed to extract metadata");

    println!("Found {} classes", metadata.len());
    for class in metadata {
        println!("\nClass: {}", class.fqcn);
        println!("  Type: {}", class.kind);
        println!("  Attributes: {} ", class.attributes.len());
        for (attr, args) in &class.attributes {
            println!("    {}: {:?}", attr, args);
        }

        println!("  Methods: {}", class.methods.len());
        for method in &class.methods {
            println!("\n    Method: {}", method.name);
            println!("      Visibility: {}", method.visibility);
            println!("      Static: {}", method.modifiers.is_static);
            println!("      Attributes: {}", method.attributes.len());
            for (attr, args) in &method.attributes {
                println!("        {}: {:?}", attr, args);
            }
            println!("      Parameters: {}", method.parameters.len());
            println!("      Return type: {:?}", method.return_type);
        }
    }
}
