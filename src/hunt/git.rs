use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use anyhow::{Context, Result};
use colored::*;

use crate::forge::{BuildDetector, MemoryGuard, PsiWatcher};

pub struct GitHunter;

impl GitHunter {
    pub fn forge_git_prey(target: &str, forge_dir: &Path) -> Result<(String, String, PathBuf)> {
        let git_url = if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("git@") {
            target.to_string()
        } else if target.contains('/') {
            format!("https://github.com/{}.git", target.trim_end_matches(".git"))
        } else {
            format!("https://github.com/{}/{}.git", target, target)
        };

        let raw_name = git_url.trim_end_matches('/').split('/').last().unwrap_or("app");
        let pkg_name = raw_name.trim_end_matches(".git").to_lowercase();

        println!("{} Resolving Git hunting grounds: '{}'", "::".magenta().bold(), git_url.cyan());

        // 1. MemoryGuard Assessment
        let profile = MemoryGuard::assess_build_target(&pkg_name);
        let zram_tag = if profile.has_zram { " [ZRAM Active]" } else { "" };
        let lto_tag = if profile.disable_lto { " [LTO Disabled for OOM Safety]" } else { " [LTO Enabled]" };

        println!(
            "{} MemoryGuard: Memory Profile Verified (Free RAM: {:.1} GB / Threads: -j{}{}{})",
            "🛡️".green().bold(),
            profile.avail_ram_mb as f64 / 1024.0,
            profile.safe_threads,
            zram_tag.cyan(),
            lto_tag.magenta()
        );

        let src_clone = forge_dir.join("src");
        let dest_install = forge_dir.join("dest");
        fs::create_dir_all(&dest_install.join("bin"))?;

        // 2. Shallow Clone
        println!("{} Cloning upstream repository...", "::".cyan().bold());
        let clone_status = Command::new("git")
            .args(["clone", "--depth", "1", &git_url, src_clone.to_str().unwrap()])
            .status()
            .with_context(|| "Failed to execute git clone")?;

        if !clone_status.success() {
            anyhow::bail!("Failed to clone git repository: {}", git_url);
        }

        // Get git commit version
        let version_out = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(&src_clone)
            .output()
            .ok();
        let version = version_out
            .map(|o| format!("git-{}", String::from_utf8_lossy(&o.stdout).trim()))
            .unwrap_or_else(|| "1.0.0".to_string());

        // 3. Detect Build System
        let plan = BuildDetector::detect(&src_clone);
        println!("{} Detected Architecture: {}", "★".yellow().bold(), plan.build_type.bold());

        // 4. Compile in isolated process group with live PSI monitoring
        println!("{} Forging binaries in RAM slab...", "::".green().bold());

        let mut build_cmd;
        if plan.build_type.contains("Rust") {
            build_cmd = Command::new("cargo");
            build_cmd.args(["build", "--release", "--locked"])
                .current_dir(&src_clone);
        } else if plan.build_type.contains("Go") {
            build_cmd = Command::new("go");
            build_cmd.args(["build", "-v", "-o", &format!("{}/bin/{}", dest_install.display(), pkg_name)])
                .current_dir(&src_clone);
        } else if plan.build_type.contains("CMake") {
            let _ = Command::new("cmake")
                .args(["-B", "build", "-DCMAKE_BUILD_TYPE=Release", &format!("-DCMAKE_INSTALL_PREFIX={}", dest_install.display())])
                .current_dir(&src_clone)
                .status();
            build_cmd = Command::new("cmake");
            build_cmd.args(["--build", "build", "-j", &profile.safe_threads.to_string()])
                .current_dir(&src_clone);
        } else {
            build_cmd = Command::new("make");
            build_cmd.args(["-j", &profile.safe_threads.to_string()])
                .current_dir(&src_clone);
        }

        build_cmd.process_group(0);

        let mut child = build_cmd.spawn()
            .with_context(|| "Forge compilation failed to spawn")?;

        let pgid = child.id() as libc::pid_t;
        let cancel_signal = Arc::new(AtomicBool::new(false));
        let psi_handle = PsiWatcher::start(pgid, cancel_signal.clone());

        let wait_status = child.wait();
        cancel_signal.store(true, Ordering::Relaxed);
        let _ = psi_handle.join();

        let build_status = wait_status.with_context(|| "Forge build execution failed")?;
        if !build_status.success() {
            anyhow::bail!("Compilation failed in forge for '{}'", pkg_name);
        }

        // 5. Populate dest directory
        if plan.build_type.contains("Rust") {
            let release_dir = src_clone.join("target/release");
            if let Ok(entries) = fs::read_dir(&release_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Ok(meta) = p.metadata() {
                                if meta.permissions().mode() & 0o111 != 0 && !p.to_string_lossy().contains(".d") {
                                    let dest_bin = dest_install.join("bin").join(p.file_name().unwrap());
                                    let _ = fs::copy(&p, dest_bin);
                                }
                            }
                        }
                    }
                }
            }
        } else if plan.build_type.contains("CMake") {
            let _ = Command::new("cmake")
                .args(["--install", "build", "--prefix", dest_install.to_str().unwrap()])
                .current_dir(&src_clone)
                .status();
        } else {
            let _ = Command::new("make")
                .args(["install", &format!("DESTDIR={}", dest_install.display())])
                .current_dir(&src_clone)
                .status();
        }

        Ok((pkg_name, version, dest_install))
    }
}
