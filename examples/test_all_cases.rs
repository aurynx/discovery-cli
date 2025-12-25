use aurynx::parser::PhpMetadataExtractor;
use std::fs;
use std::path::PathBuf;

fn main() {
    let mut extractor = PhpMetadataExtractor::new().expect("Failed to create extractor");

    let test_files = [
        ("01_simple_method.php", "Simple method"),
        ("02_method_with_attribute.php", "Method with attribute"),
        ("03_method_with_params.php", "Method with parameters"),
        (
            "04_method_with_param_attribute.php",
            "Method with parameter attribute",
        ),
        ("05_static_abstract_final.php", "Static, abstract, final"),
        ("06_multiple_attributes.php", "Multiple attributes"),
    ];

    for (file, desc) in &test_files {
        println!("\n{}", "=".repeat(70));
        println!("TEST: {} ({})", desc, file);
        println!("{}\n", "=".repeat(70));

        let path = format!("../examples/test_cases/{}", file);
        let code = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                println!("❌ Failed to read file: {}", e);
                continue;
            }
        };

        let metadata = match extractor.extract_metadata(&code, PathBuf::from(file)) {
            Ok(m) => m,
            Err(e) => {
                println!("❌ Failed to extract metadata: {}", e);
                continue;
            }
        };

        for class in metadata {
            println!("📦 Class: {}", class.fqcn);
            println!("   Type: {}", class.kind);
            println!(
                "   Modifiers: abstract={}, final={}, readonly={}",
                class.modifiers.is_abstract, class.modifiers.is_final, class.modifiers.is_readonly
            );

            if !class.attributes.is_empty() {
                println!("   Class Attributes:");
                for (attr, args) in &class.attributes {
                    println!("     - {}: {:?}", attr, args);
                }
            }

            println!("\n   Methods: {}", class.methods.len());
            for method in &class.methods {
                println!("\n   🔧 {}", method.name);
                println!("      Visibility: {}", method.visibility);
                println!(
                    "      Modifiers: abstract={}, final={}, static={}",
                    method.modifiers.is_abstract,
                    method.modifiers.is_final,
                    method.modifiers.is_static
                );

                if !method.attributes.is_empty() {
                    println!("      Attributes:");
                    for (attr, args) in &method.attributes {
                        println!("        - {}: {:?}", attr, args);
                    }
                }

                if !method.parameters.is_empty() {
                    println!("      Parameters:");
                    for param in &method.parameters {
                        let type_str = param
                            .type_hint
                            .as_ref()
                            .map(|t| format!(": {}", t))
                            .unwrap_or_default();
                        let default_str = param
                            .default_value
                            .as_ref()
                            .map(|d| format!(" = {}", d))
                            .unwrap_or_default();

                        let attrs_str = if !param.attributes.is_empty() {
                            let attrs: Vec<String> =
                                param.attributes.keys().map(|k| k.to_string()).collect();
                            format!(" #[{}]", attrs.join(", "))
                        } else {
                            String::new()
                        };

                        println!(
                            "        - {}{}{}{}",
                            param.name, type_str, default_str, attrs_str
                        );
                    }
                }

                if let Some(ret_type) = &method.return_type {
                    println!("      Return: {}", ret_type);
                }
            }

            println!();
        }
    }

    println!("\n✅ All tests completed");
}
