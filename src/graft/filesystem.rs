use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use colored::*;

use crate::mutator::soname::SonameScanner;
use crate::mutator::ElfMutator;

#[derive(Debug, Clone)]
pub struct GraftResult {
    pub binary_paths: Vec<String>,
    pub companion_libs: Vec<String>,
    pub desktop_files: Vec<String>,
}

pub struct GraftEngine;

impl GraftEngine {
    pub fn get_mimic_root() -> PathBuf {
        let global = Path::new("/mimic");
        if global.exists() || is_writable(global) {
            return global.to_path_buf();
        }

        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local/mimic")
        } else {
            PathBuf::from("/tmp/mimic")
        }
    }

    pub fn graft_payload(raw_dir: &Path, package_id: &str) -> Result<GraftResult> {
        let mimic_root = Self::get_mimic_root();
        let bin_dir = mimic_root.join("bin");
        let companion_dir = mimic_root.join("lib").join(package_id);
        let share_dir = mimic_root.join("share");
        let apps_dir = share_dir.join("applications");
        let icons_dir = share_dir.join("icons");

        fs::create_dir_all(&bin_dir)?;
        fs::create_dir_all(&companion_dir)?;
        fs::create_dir_all(&apps_dir)?;
        fs::create_dir_all(&icons_dir)?;

        let mut binary_paths = Vec::new();
        let mut companion_libs = Vec::new();
        let mut desktop_files = Vec::new();

        // 1. Traverse raw extracted payload
        let mut all_files = Vec::new();
        collect_files_recursive(raw_dir, &mut all_files)?;

        // First pass: Scavenge companion shared libraries (.so)
        for src_path in &all_files {
            let file_name = src_path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.contains(".so") {
                let dest = companion_dir.join(&*file_name);
                fs::copy(src_path, &dest)?;
                companion_libs.push(dest.to_string_lossy().to_string());
            }
        }

        // Second pass: Find executables & mutate ELF headers
        for src_path in &all_files {
            let file_name = src_path.file_name().unwrap_or_default().to_string_lossy();
            let is_in_bin = src_path.to_string_lossy().contains("/bin/")
                || src_path.to_string_lossy().contains("/sbin/")
                || src_path.to_string_lossy().contains("/opt/");

            if is_in_bin && !file_name.contains(".so") && !src_path.is_dir() {
                let dest_bin = bin_dir.join(&*file_name);
                fs::copy(src_path, &dest_bin)?;

                if SonameScanner::is_elf(&dest_bin) {
                    // Mutate the ELF headers: rewrite DT_RUNPATH to companion pockets
                    if let Ok(analysis) = ElfMutator::mutate_binary(&dest_bin, package_id, Some(&companion_dir)) {
                        if !analysis.missing_libraries.is_empty() {
                            println!(
                                "  {} Warning: Missing {} unresolved libraries on host: {:?}",
                                "⚠️".yellow(),
                                analysis.missing_libraries.len(),
                                analysis.missing_libraries
                            );
                        }
                    }
                }

                binary_paths.push(dest_bin.to_string_lossy().to_string());

                // If userland root, also ensure symlink in ~/.local/bin
                if let Ok(home) = std::env::var("HOME") {
                    let local_bin = PathBuf::from(home).join(".local/bin");
                    let _ = fs::create_dir_all(&local_bin);
                    let symlink_path = local_bin.join(&*file_name);
                    let _ = fs::remove_file(&symlink_path);
                    let _ = std::os::unix::fs::symlink(&dest_bin, &symlink_path);
                }
            } else if file_name.ends_with(".desktop") {
                let dest_desktop = apps_dir.join(&*file_name);
                Self::patch_and_copy_desktop(src_path, &dest_desktop, &bin_dir)?;
                desktop_files.push(dest_desktop.to_string_lossy().to_string());

                // Also copy to ~/.local/share/applications for instant Noctalia/DE pickup
                if let Ok(home) = std::env::var("HOME") {
                    let user_apps = PathBuf::from(home).join(".local/share/applications");
                    let _ = fs::create_dir_all(&user_apps);
                    let user_desktop = user_apps.join(&*file_name);
                    let _ = fs::copy(&dest_desktop, user_desktop);
                }
            } else if src_path.to_string_lossy().contains("/icons/") || src_path.to_string_lossy().contains("/pixmaps/") {
                if let Some(parent) = src_path.parent() {
                    let rel_sub = parent.strip_prefix(raw_dir).unwrap_or(parent);
                    let dest_icon_parent = share_dir.join(rel_sub);
                    let _ = fs::create_dir_all(&dest_icon_parent);
                    let dest_icon = dest_icon_parent.join(&*file_name);
                    let _ = fs::copy(src_path, dest_icon);
                }
            }
        }

        Ok(GraftResult {
            binary_paths,
            companion_libs,
            desktop_files,
        })
    }

    fn patch_and_copy_desktop(src: &Path, dest: &Path, bin_dir: &Path) -> Result<()> {
        let content = fs::read_to_string(src).unwrap_or_default();
        let mut patched = Vec::new();

        for line in content.lines() {
            if let Some(exec_cmd) = line.strip_prefix("Exec=") {
                let parts: Vec<&str> = exec_cmd.split_whitespace().collect();
                if let Some(first) = parts.first() {
                    let binary_name = Path::new(first).file_name().unwrap_or_default().to_string_lossy();
                    let target_bin = bin_dir.join(&*binary_name);
                    let rest = parts[1..].join(" ");
                    patched.push(format!("Exec={} {}", target_bin.display(), rest));
                } else {
                    patched.push(line.to_string());
                }
            } else {
                patched.push(line.to_string());
            }
        }

        fs::write(dest, patched.join("\n"))?;
        Ok(())
    }
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, files)?;
            } else {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn is_writable(path: &Path) -> bool {
    if path.exists() {
        if let Ok(meta) = fs::metadata(path) {
            return !meta.permissions().readonly();
        }
    }
    false
}
