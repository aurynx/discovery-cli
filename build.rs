// Build script for creating 'anx' symlink to 'aurynx' binary
//
// This allows users to use either:
//   aurynx discovery:scan --path src/
// or the shorter alias:
//   anx discovery:scan --path src/

#![allow(clippy::expect_used)]

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Get the output directory (target/release or target/debug)
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let profile = env::var("PROFILE").expect("PROFILE not set");

    // Build the target directory path
    let mut target_dir = PathBuf::from(&out_dir);
    // OUT_DIR is deep (target/release/build/aurynx-.../out), go up to target/release
    while target_dir.file_name().and_then(|n| n.to_str()) != Some(&profile) {
        if !target_dir.pop() {
            eprintln!("Warning: Could not find target profile directory");
            return;
        }
    }

    let aurynx_binary = target_dir.join("aurynx");
    let anx_symlink = target_dir.join("anx");

    // Only create symlink on Unix-like systems
    #[cfg(unix)]
    {
        // Remove existing symlink if present
        if anx_symlink.exists() || anx_symlink.symlink_metadata().is_ok() {
            let _ = fs::remove_file(&anx_symlink);
        }

        // Create symlink
        if aurynx_binary.exists() {
            match std::os::unix::fs::symlink("aurynx", &anx_symlink) {
                Ok(()) => println!("cargo:warning=Created symlink: anx -> aurynx"),
                Err(e) => eprintln!("Warning: Failed to create 'anx' symlink: {e}"),
            }
        }
    }

    #[cfg(windows)]
    {
        println!("cargo:warning=Symlink creation not supported on Windows");
        println!("cargo:warning=Use 'aurynx' directly or create a batch file alias");
    }

    // Rebuild if build.rs changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    // Git Commit Hash
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();
    let git_hash = match output {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout)
            .unwrap_or_default()
            .trim()
            .to_string(),
        _ => "unknown".to_string(),
    };
    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    // Git Commit Date
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%cd", "--date=short"])
        .output();
    let commit_date = match output {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout)
            .unwrap_or_default()
            .trim()
            .to_string(),
        _ => {
            // Fallback to current date on Unix
            #[cfg(unix)]
            {
                let output = std::process::Command::new("date").arg("+%Y-%m-%d").output();
                match output {
                    Ok(o) if o.status.success() => String::from_utf8(o.stdout)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    _ => "unknown".to_string(),
                }
            }
            #[cfg(not(unix))]
            "unknown".to_string()
        },
    };
    println!("cargo:rustc-env=COMMIT_DATE={commit_date}");

    // Target Architecture
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=TARGET={target}");
}
