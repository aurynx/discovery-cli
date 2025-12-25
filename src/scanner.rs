use crate::metadata::PhpClassMetadata;
use crate::parser::PhpMetadataExtractor;
use ignore::{WalkBuilder, WalkState};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use tracing::{error, warn};

/// Default maximum file size allowed for parsing (10MB)
/// Files larger than this will be skipped to prevent OOM
/// Can be overridden via config file
pub const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

#[must_use] 
pub fn scan_directory(paths: &[PathBuf], ignored: &[String]) -> Vec<PhpClassMetadata> {
    scan_directory_with_limit(paths, ignored, DEFAULT_MAX_FILE_SIZE)
}

/// Scan directory with custom file size limit
pub fn scan_directory_with_limit(
    paths: &[PathBuf], ignored: &[String], max_file_size: u64,
) -> Vec<PhpClassMetadata> {
    if paths.is_empty() {
        return vec![];
    }

    let mut builder = WalkBuilder::new(&paths[0]);
    for path in &paths[1..] {
        builder.add(path);
    }

    let mut overrides = ignore::overrides::OverrideBuilder::new(&paths[0]);
    for ignore in ignored {
        if let Err(e) = overrides.add(&format!("!{ignore}")) {
            warn!("Invalid ignore pattern '{}': {}", ignore, e);
        }
    }

    if let Ok(ov) = overrides.build() {
        builder.overrides(ov);
    }

    builder.git_ignore(true);

    let (tx, rx) = channel();

    builder.build_parallel().run(|| {
        let tx = tx.clone();
        let mut extractor = match PhpMetadataExtractor::new() {
            Ok(e) => Some(e),
            Err(e) => {
                error!("Error creating metadata extractor: {}", e);
                None
            },
        };

        Box::new(move |entry| {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => return WalkState::Continue,
            };

            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                return WalkState::Continue;
            }

            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "php")
                && let Some(extractor) = &mut extractor {
                    // Check file size before reading to prevent OOM
                    match fs::metadata(path) {
                        Ok(metadata) => {
                            let file_size = metadata.len();
                            if file_size > max_file_size {
                                warn!(
                                    "Skipping large file: {:?} ({:.2}MB exceeds limit of {:.2}MB)",
                                    path,
                                    file_size as f64 / 1024.0 / 1024.0,
                                    max_file_size as f64 / 1024.0 / 1024.0
                                );
                                return WalkState::Continue;
                            }
                        },
                        Err(e) => {
                            warn!("Could not read metadata for {:?}: {}", path, e);
                            return WalkState::Continue;
                        },
                    }

                    if let Ok(content) = fs::read_to_string(path) {
                        match extractor.extract_metadata(&content, path.to_path_buf()) {
                            Ok(metadata_list) => {
                                for metadata in metadata_list {
                                    let _ = tx.send(metadata);
                                }
                            },
                            Err(e) => {
                                error!("Error parsing file {:?}: {}", path, e);
                            },
                        }
                    }
                }

            WalkState::Continue
        })
    });

    drop(tx);

    let mut results: Vec<PhpClassMetadata> = rx.into_iter().collect();
    results.sort_by(|a, b| a.fqcn.cmp(&b.fqcn));
    results
}

/// Scan only specific files (for incremental updates)
#[must_use] 
pub fn scan_files(files: &[PathBuf]) -> Vec<PhpClassMetadata> {
    scan_files_with_limit(files, DEFAULT_MAX_FILE_SIZE)
}

/// Scan specific files with custom file size limit
pub fn scan_files_with_limit(files: &[PathBuf], max_file_size: u64) -> Vec<PhpClassMetadata> {
    let mut results = Vec::new();

    let mut extractor = match PhpMetadataExtractor::new() {
        Ok(e) => e,
        Err(e) => {
            error!("Error creating metadata extractor: {}", e);
            return vec![];
        },
    };

    for path in files {
        if !path.exists() || !path.is_file() {
            continue;
        }

        if path.extension().is_some_and(|ext| ext == "php") {
            // Check file size before reading to prevent OOM
            match fs::metadata(path) {
                Ok(metadata) => {
                    let file_size = metadata.len();
                    if file_size > max_file_size {
                        warn!(
                            "Skipping large file: {:?} ({:.2}MB exceeds limit of {:.2}MB)",
                            path,
                            file_size as f64 / 1024.0 / 1024.0,
                            max_file_size as f64 / 1024.0 / 1024.0
                        );
                        continue;
                    }
                },
                Err(e) => {
                    warn!("Could not read metadata for {:?}: {}", path, e);
                    continue;
                },
            }

            if let Ok(content) = fs::read_to_string(path) {
                match extractor.extract_metadata(&content, path.clone()) {
                    Ok(metadata_list) => {
                        results.extend(metadata_list);
                    },
                    Err(e) => {
                        error!("Error parsing file {:?}: {}", path, e);
                    },
                }
            }
        }
    }

    results.sort_by(|a, b| a.fqcn.cmp(&b.fqcn));
    results
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_scan_directory() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // File with attribute
        let file1 = root.join("WithAttr.php");
        let mut f1 = File::create(&file1).unwrap();
        writeln!(f1, "<?php namespace App; #[Attribute] class A {{}}").unwrap();

        // File without attribute
        let file2 = root.join("NoAttr.php");
        let mut f2 = File::create(&file2).unwrap();
        writeln!(f2, "<?php namespace App; class B {{}}").unwrap();

        // Ignored file (simulated by passing ignore list)
        let file3 = root.join("Ignored.php");
        let mut f3 = File::create(&file3).unwrap();
        writeln!(f3, "<?php namespace App; #[Attribute] class C {{}}").unwrap();

        // Non-PHP file
        let file4 = root.join("other.txt");
        let mut f4 = File::create(&file4).unwrap();
        writeln!(f4, "content").unwrap();

        let paths = vec![root.to_path_buf()];
        let ignored = vec!["Ignored.php".to_string()];

        let results = scan_directory(&paths, &ignored);

        // Should contain both classes (with and without attributes)
        assert!(results.len() >= 2);

        let fqcns: Vec<String> = results.iter().map(|m| m.fqcn.clone()).collect();
        assert!(fqcns.contains(&"\\App\\A".to_string()));
        assert!(fqcns.contains(&"\\App\\B".to_string()));
        assert!(!fqcns.contains(&"\\App\\C".to_string())); // Should be ignored
    }
}
