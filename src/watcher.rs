use crate::metadata::PhpClassMetadata;
use crate::parser::PhpMetadataExtractor;
use crate::scanner::scan_directory;
use crate::writer::write_php_cache;
use dashmap::DashMap;
use ignore::gitignore::GitignoreBuilder;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, warn};

pub fn watch_directory(paths: &[PathBuf], ignored: &[String], output: &Path) -> anyhow::Result<()> {
    // Canonicalize paths to resolve symlinks (important for macOS /tmp -> /private/tmp)
    let paths: Vec<PathBuf> = paths
        .iter()
        .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect();
    let paths = &paths; // Shadow the argument with reference to local vector

    println!("Performing initial scan...");
    let metadata = scan_directory(paths, ignored);

    let mut ignore_builder = GitignoreBuilder::new(&paths[0]);
    for ignore in ignored {
        if let Err(e) = ignore_builder.add_line(None, ignore) {
            warn!("Invalid ignore pattern '{}': {}", ignore, e);
        }
    }
    let ignore_matcher = ignore_builder.build()?;

    // State: map of file path -> list of metadata for that file
    let state: Arc<DashMap<PathBuf, Vec<PhpClassMetadata>>> = Arc::new(DashMap::new());
    for meta in &metadata {
        state
            .entry(meta.file.clone())
            .or_default()
            .push(meta.clone());
    }

    write_php_cache(&metadata, output, true)?;
    println!(
        "Initial scan complete. Found {} classes/interfaces/traits/enums.",
        metadata.len()
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(std::time::Duration::from_millis(200), tx)?;

    for path in paths {
        debouncer.watcher().watch(path, RecursiveMode::Recursive)?;
        println!("Watching for changes in {path:?}...");
    }

    for res in rx {
        match res {
            Ok(events) => {
                let mut changed = false;
                for event in events {
                    let path = event.path;
                    let relative_path = match path.strip_prefix(&paths[0]) {
                        Ok(p) => p,
                        Err(_) => &path,
                    };

                    if ignore_matcher.matched(relative_path, false).is_ignore() {
                        continue;
                    }

                    if path.extension().is_some_and(|ext| ext == "php") {
                        if path.exists() {
                            // File created or modified
                            if let Ok(content) = fs::read_to_string(&path) {
                                let mut extractor = match PhpMetadataExtractor::new() {
                                    Ok(e) => e,
                                    Err(e) => {
                                        error!("Error creating extractor: {}", e);
                                        continue;
                                    }
                                };

                                match extractor.extract_metadata(&content, path.clone()) {
                                    Ok(new_metadata) => {
                                        let old_metadata = state.get(&path).map(|v| v.clone());

                                        // Check if metadata changed
                                        let metadata_changed = match old_metadata {
                                            Some(ref old) => old != &new_metadata,
                                            None => !new_metadata.is_empty(),
                                        };

                                        if metadata_changed {
                                            if new_metadata.is_empty() {
                                                state.remove(&path);
                                            } else {
                                                state.insert(path.clone(), new_metadata);
                                            }
                                            changed = true;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Error parsing {:?}: {}", path, e);
                                    }
                                }
                            }
                        } else {
                            // File removed
                            if state.remove(&path).is_some() {
                                changed = true;
                            }
                        }
                    }
                }

                if changed {
                    println!("Changes detected, updating cache...");

                    let mut all_metadata: Vec<PhpClassMetadata> = Vec::new();
                    for entry in state.iter() {
                        all_metadata.extend(entry.value().clone());
                    }

                    all_metadata.sort_by(|a, b| a.fqcn.cmp(&b.fqcn));

                    if let Err(e) = write_php_cache(&all_metadata, output, true) {
                        error!("Error writing cache: {}", e);
                    }
                }
            }
            Err(e) => error!("Watch error: {:?}", e),
        }
    }

    Ok(())
}
