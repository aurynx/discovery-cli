use crate::metadata::PhpClassMetadata;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Manifest file name
pub const MANIFEST_FILE: &str = "aurynx.meta.json";

/// Information about a file in the manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub mtime: u64,
    pub classes: Vec<PhpClassMetadata>,
}

/// Manifest structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub files: HashMap<String, FileEntry>,
}

impl Manifest {
    /// Load manifest from file
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        let manifest = serde_json::from_str(&content).context("Failed to parse manifest file")?;
        Ok(manifest)
    }

    /// Save manifest to file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

/// Perform incremental scan using manifest
pub fn perform_incremental_scan(
    manifest_path: &Path,
    scan_paths: &[PathBuf],
    ignore_patterns: &[String],
    max_file_size: u64,
) -> Result<(Vec<PhpClassMetadata>, Manifest)> {
    // Load existing manifest
    let mut manifest = Manifest::load(manifest_path)?;

    // Collect current files
    let current_files = collect_php_files(scan_paths, ignore_patterns)?;
    let current_files_set: HashSet<String> = current_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let mut changed_files = Vec::new();
    let mut removed_files = Vec::new();

    // Check for removed files
    let cached_paths: Vec<String> = manifest.files.keys().cloned().collect();
    for cached_path in cached_paths {
        if !current_files_set.contains(&cached_path) {
            removed_files.push(cached_path);
        }
    }

    // Remove deleted files from manifest
    for path in &removed_files {
        manifest.files.remove(path);
    }

    // Check for changed or new files
    for path in current_files {
        let path_str = path.to_string_lossy().to_string();
        let mtime = fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0);

        if let Some(entry) = manifest.files.get(&path_str) {
            if mtime > entry.mtime {
                changed_files.push(path);
            }
        } else {
            // New file
            changed_files.push(path);
        }
    }

    println!(
        "Incremental scan: {} changed/new, {} removed",
        changed_files.len(),
        removed_files.len()
    );

    // Scan changed files
    if !changed_files.is_empty() {
        let new_metadata = crate::scanner::scan_files_with_limit(&changed_files, max_file_size);

        // Group metadata by file
        let mut file_metadata_map: HashMap<String, Vec<PhpClassMetadata>> = HashMap::new();
        for meta in new_metadata {
            let file_path = meta.file.to_string_lossy().to_string();
            file_metadata_map.entry(file_path).or_default().push(meta);
        }

        // Update manifest
        for path in changed_files {
            let path_str = path.to_string_lossy().to_string();
            let mtime = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(|t| {
                    t.duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
                .unwrap_or(0);

            let classes = file_metadata_map.remove(&path_str).unwrap_or_default();

            manifest
                .files
                .insert(path_str, FileEntry { mtime, classes });
        }
    }

    // Flatten manifest to list of metadata
    let all_metadata: Vec<PhpClassMetadata> = manifest
        .files
        .values()
        .flat_map(|entry| entry.classes.clone())
        .collect();

    Ok((all_metadata, manifest))
}

/// Collect all PHP files in the given paths (without parsing them)
fn collect_php_files(paths: &[PathBuf], ignored: &[String]) -> Result<Vec<PathBuf>> {
    use ignore::WalkBuilder;

    let mut files = Vec::new();

    if paths.is_empty() {
        return Ok(files);
    }

    let mut builder = WalkBuilder::new(&paths[0]);
    for path in &paths[1..] {
        builder.add(path);
    }

    let mut overrides = ignore::overrides::OverrideBuilder::new(&paths[0]);
    for ignore in ignored {
        if let Err(e) = overrides.add(&format!("!{ignore}")) {
            eprintln!("Warning: Invalid ignore pattern '{ignore}': {e}");
        }
    }

    if let Ok(ov) = overrides.build() {
        builder.overrides(ov);
    }

    builder.git_ignore(true);

    for entry in builder.build() {
        if let Ok(entry) = entry
            && entry.file_type().is_some_and(|ft| ft.is_file()) {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "php") {
                    files.push(path.to_path_buf());
                }
            }
    }

    Ok(files)
}
