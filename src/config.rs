use crate::error::{AurynxError, Result};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
pub struct ConfigFile {
    pub paths: Option<Vec<PathBuf>>,
    pub output: Option<PathBuf>,
    pub ignore: Option<Vec<String>>,
    pub watch: Option<bool>,
    pub socket: Option<PathBuf>,
    pub pid: Option<PathBuf>,
    pub incremental: Option<bool>,
    pub verbose: Option<bool>,
    pub log_file: Option<PathBuf>,
    pub log_level: Option<String>,
    pub log_format: Option<String>,
    pub force: Option<bool>,
    pub write_to_disk: Option<bool>,
    pub pretty: Option<bool>,

    // Security and performance limits
    pub max_file_size_mb: Option<u64>, // Maximum PHP file size in MB (default: 10MB)
    pub max_request_size: Option<usize>, // Maximum IPC request size in bytes (default: 1KB)
    pub max_cache_entries: Option<usize>, // Maximum number of cached classes (default: 50,000)
}

impl ConfigFile {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let config_path = if let Some(p) = path {
            if !p.exists() {
                return Err(AurynxError::config_error(format!(
                    "Config file not found: {p:?}"
                )));
            }
            Some(p)
        } else {
            // Try default locations
            let json_path = PathBuf::from("aurynx.json");
            if json_path.exists() {
                Some(json_path)
            } else {
                None
            }
        };

        if let Some(path) = config_path {
            let content = fs::read_to_string(&path).map_err(|e| {
                AurynxError::io_error(format!("Failed to read config file: {path:?}"), e)
            })?;

            let config: Self = serde_json::from_str(&content).map_err(|e| {
                AurynxError::json_error(format!("Failed to parse config file: {path:?}"), e)
            })?;

            config.validate()?;

            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(level) = &self.log_level {
            let valid_levels = ["trace", "debug", "info", "warn", "error"];
            if !valid_levels.contains(&level.as_str()) {
                return Err(AurynxError::config_error(format!(
                    "Invalid log_level: '{level}'. Allowed: {valid_levels:?}"
                )));
            }
        }

        if let Some(format) = &self.log_format {
            let valid_formats = ["text", "json"];
            if !valid_formats.contains(&format.as_str()) {
                return Err(AurynxError::config_error(format!(
                    "Invalid log_format: '{format}'. Allowed: {valid_formats:?}"
                )));
            }
        }

        // Validate limits
        if let Some(size) = self.max_file_size_mb {
            if size == 0 {
                return Err(AurynxError::config_error(
                    "max_file_size_mb must be greater than 0",
                ));
            }
            if size > 1024 {
                return Err(AurynxError::config_error(format!(
                    "max_file_size_mb too large: {size}MB (maximum: 1024MB / 1GB)"
                )));
            }
        }

        if let Some(size) = self.max_request_size {
            if size < 256 {
                return Err(AurynxError::config_error(format!(
                    "max_request_size too small: {size} bytes (minimum: 256 bytes)"
                )));
            }
            if size > 1024 * 1024 {
                return Err(AurynxError::config_error(format!(
                    "max_request_size too large: {size} bytes (maximum: 1MB)"
                )));
            }
        }

        if let Some(entries) = self.max_cache_entries {
            if entries == 0 {
                return Err(AurynxError::config_error(
                    "max_cache_entries must be greater than 0",
                ));
            }
            if entries > 1_000_000 {
                return Err(AurynxError::config_error(format!(
                    "max_cache_entries too large: {entries} (maximum: 1,000,000)"
                )));
            }
        }

        Ok(())
    }

    /// Get max file size in bytes (default: 10MB)
    #[must_use] 
    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size_mb.unwrap_or(10) * 1024 * 1024
    }

    /// Get max request size in bytes (default: 1KB)
    #[must_use] 
    pub fn max_request_size_bytes(&self) -> usize {
        self.max_request_size.unwrap_or(1024)
    }

    /// Get max cache entries (default: 50,000)
    #[must_use] 
    pub fn max_cache_entries_limit(&self) -> usize {
        self.max_cache_entries.unwrap_or(50_000)
    }
}
