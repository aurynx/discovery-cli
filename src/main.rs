use aurynx::daemon::{Daemon, DaemonConfig};
use aurynx::scanner::scan_directory;
use aurynx::writer::write_php_cache;
use clap::{Parser, Subcommand};
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "aurynx",
    author,
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), " ", env!("COMMIT_DATE"), ") ", env!("TARGET")),
    about = "Aurynx CLI - PHP attribute discovery and code analysis",
    long_about = "Unified CLI for Aurynx framework tools. Use 'discovery:scan' for PHP attribute discovery."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// PHP attribute discovery and metadata extraction
    #[command(name = "discovery:scan", visible_alias = "discovery")]
    DiscoveryScan {
        /// Configuration file path (defaults to aurynx.json)
        #[arg(long)]
        config: Option<PathBuf>,

        /// Directories to scan for PHP files
        #[arg(short, long, num_args = 1..)]
        path: Option<Vec<PathBuf>>,

        /// Output cache file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Ignore patterns (can be used multiple times, e.g., --ignore "vendor/*" --ignore "tests/*")
        #[arg(short, long)]
        ignore: Option<Vec<String>>,

        /// Watch for file changes and run as daemon (requires --socket and --pid)
        #[arg(short, long)]
        watch: bool,

        /// Unix socket path for IPC (required with --watch)
        #[arg(short, long)]
        socket: Option<PathBuf>,

        /// PID file path (required with --watch)
        #[arg(long)]
        pid: Option<PathBuf>,

        /// Incremental mode: only rescan changed files (scan mode only)
        #[arg(long, conflicts_with = "watch")]
        incremental: bool,

        /// Verbose logging (watch mode only)
        #[arg(short, long)]
        verbose: bool,

        /// Log file path (optional, defaults to stdout)
        #[arg(long)]
        log_file: Option<PathBuf>,

        /// Log level: trace, debug, info, warn, error
        #[arg(long)]
        log_level: Option<String>,

        /// Log format: text or json
        #[arg(long)]
        log_format: Option<String>,

        /// Force restart even if daemon is already running (DANGEROUS: kills existing daemon)
        #[arg(long)]
        force: bool,

        /// Force writing cache to disk in watch mode (useful for debugging/testing)
        #[arg(long)]
        write_to_disk: bool,

        /// Pretty print output (formatted with indentation)
        #[arg(long)]
        pretty: bool,

        /// Output format (currently only 'php' is supported)
        #[arg(long, default_value = "php", hide = true)]
        format: String,

        /// Include attributes in output (enabled by default)
        #[arg(long, default_value = "true", hide = true)]
        include_attributes: bool,

        /// Include parent classes and interfaces (enabled by default)
        #[arg(long, default_value = "true", hide = true)]
        include_parents: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::DiscoveryScan {
            config: config_path,
            path,
            output,
            ignore,
            watch,
            socket,
            pid,
            incremental,
            verbose,
            log_file,
            log_level,
            log_format,
            force,
            write_to_disk,
            pretty,
            format,
            include_attributes: _,
            include_parents: _,
        } => {
            // Load config file
            let config_file = match aurynx::config::ConfigFile::load(config_path.clone()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {e}");
                    std::process::exit(1);
                },
            };

            // Extract limit settings before moving config_file
            let max_file_size = config_file.max_file_size_bytes();
            let max_request_size = config_file.max_request_size_bytes();
            let max_cache_entries = config_file.max_cache_entries_limit();

            // Merge config (CLI args > Config file > Defaults)
            let path = path.clone().or(config_file.paths).unwrap_or_else(|| {
                eprintln!("Error: --path is required (or 'paths' in config file)");
                std::process::exit(1);
            });

            let output = output.clone().or(config_file.output).unwrap_or_else(|| {
                eprintln!("Error: --output is required (or 'output' in config file)");
                std::process::exit(1);
            });

            let ignore = ignore.clone().or(config_file.ignore).unwrap_or_default();
            let watch = *watch || config_file.watch.unwrap_or(false);
            let socket = socket.clone().or(config_file.socket);
            let pid = pid.clone().or(config_file.pid);
            let incremental = *incremental || config_file.incremental.unwrap_or(false);
            let verbose = *verbose || config_file.verbose.unwrap_or(false);
            let log_file = log_file.clone().or(config_file.log_file);
            let log_level = log_level
                .clone()
                .or(config_file.log_level)
                .unwrap_or_else(|| "info".to_string());
            let log_format = log_format
                .clone()
                .or(config_file.log_format)
                .unwrap_or_else(|| "text".to_string());
            let force = *force || config_file.force.unwrap_or(false);
            let write_to_disk = *write_to_disk || config_file.write_to_disk.unwrap_or(false);
            let pretty = *pretty || config_file.pretty.unwrap_or(false);

            // Validate format
            if format != "php" && format != "json" {
                eprintln!("Error: Only 'php' and 'json' formats are supported");
                std::process::exit(1);
            }

            // WATCH MODE (daemon)
            if watch {
                // Validate required arguments
                let socket_path = if let Some(s) = socket.as_ref() { s } else {
                    eprintln!("Error: --socket is required with --watch (or in config)");
                    std::process::exit(1);
                };
                let pid_path = if let Some(p) = pid.as_ref() { p } else {
                    eprintln!("Error: --pid is required with --watch (or in config)");
                    std::process::exit(1);
                };

                // Initialize logger
                let is_tty = std::io::stdout().is_terminal();
                if let Err(e) = aurynx::logger::init_logger(
                    log_file.as_deref(),
                    &log_level,
                    &log_format,
                    verbose,
                ) {
                    eprintln!("❌ Failed to initialize logger: {e}");
                    std::process::exit(1);
                }

                // Show startup info if interactive
                if is_tty {
                    println!("🪄 Starting Discovery daemon...");
                    println!("   Mode: Watch (with atomic lock)");
                    println!("   Strategy: Adaptive caching");
                    println!("   Paths: {path:?}");
                    println!("   Output: {output:?}");
                    println!("   Socket: {socket_path:?}");
                    println!("   PID: {pid_path:?}");
                    if verbose {
                        println!("   Verbose: enabled 🔮");
                    }
                    if let Some(lf) = &log_file {
                        println!("   Log file: {lf:?}");
                        println!("   Log format: {log_format}");
                    }
                }

                // Create daemon config
                let config = DaemonConfig {
                    paths: path,
                    output_path: output,
                    socket_path: socket_path.clone(),
                    pid_file: pid_path.clone(),
                    ignore_patterns: ignore,
                    verbose,
                    is_tty,
                    force,
                    write_to_disk,
                    pretty,
                    format: format.clone(),
                    max_file_size,
                    max_request_size,
                    max_cache_entries,
                };

                // Start daemon
                let mut daemon = match Daemon::new(config) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Failed to create daemon: {e}");
                        std::process::exit(1);
                    },
                };

                if let Err(e) = daemon.run() {
                    eprintln!("Daemon error: {e}");
                    std::process::exit(1);
                }
            }
            // SCAN MODE (one-shot)
            else {
                println!("Scanning {path:?} -> {output:?} (ignoring {ignore:?})");

                let manifest_path = if let Some(parent) = output.parent() {
                    parent.join(aurynx::incremental::MANIFEST_FILE)
                } else {
                    PathBuf::from(aurynx::incremental::MANIFEST_FILE)
                };

                // Incremental or full scan
                let (metadata, manifest) = if incremental {
                    match aurynx::incremental::perform_incremental_scan(
                        &manifest_path,
                        &path,
                        &ignore,
                        max_file_size,
                    ) {
                        Ok(res) => res,
                        Err(e) => {
                            eprintln!(
                                "Warning: Incremental mode failed, falling back to full scan: {e}"
                            );
                            let meta = scan_directory(&path, &ignore);
                            (meta, aurynx::incremental::Manifest::default())
                        },
                    }
                } else {
                    let meta = scan_directory(&path, &ignore);
                    match aurynx::incremental::perform_incremental_scan(
                        &PathBuf::from("/non-existent"), // Force full scan
                        &path,
                        &ignore,
                        max_file_size,
                    ) {
                        Ok(res) => res,
                        Err(_) => (meta, aurynx::incremental::Manifest::default()),
                    }
                };

                println!("Found {} classes/interfaces/traits/enums.", metadata.len());

                // Write cache
                let result = match format.as_str() {
                    "json" => aurynx::writer::write_json_cache(&metadata, &output, pretty),
                    _ => write_php_cache(&metadata, &output, pretty),
                };

                if let Err(e) = result {
                    eprintln!("Error writing cache: {e}");
                    std::process::exit(1);
                }

                // Write manifest
                if let Err(e) = manifest.save(&manifest_path) {
                    eprintln!("Warning: Failed to save manifest: {e}");
                }

                println!("Cache written successfully to {output:?}");
            }
        },
    }
}
