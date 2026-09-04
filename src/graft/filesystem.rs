use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use anyhow::Result;
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
        let real_bin_dir = bin_dir.join(".real");
        let companion_dir = mimic_root.join("lib").join(package_id);
        let share_dir = mimic_root.join("share");
        let opt_dir = mimic_root.join("opt").join(package_id);
        let apps_dir = share_dir.join("applications");
        let icons_dir = share_dir.join("icons");

        fs::create_dir_all(&bin_dir)?;
        fs::create_dir_all(&real_bin_dir)?;
        fs::create_dir_all(&companion_dir)?;
        fs::create_dir_all(&share_dir)?;
        fs::create_dir_all(&opt_dir)?;
        fs::create_dir_all(&apps_dir)?;
        fs::create_dir_all(&icons_dir)?;

        let mut binary_paths = Vec::new();
        let mut companion_libs = Vec::new();
        let mut desktop_files = Vec::new();

        let mut all_files = Vec::new();
        collect_files_recursive(raw_dir, &mut all_files)?;

        // 1. Scavenge all companion shared libraries (.so) and set 0o755
        for src_path in &all_files {
            let file_name = src_path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.contains(".so") {
                let dest = companion_dir.join(&*file_name);
                let _ = fs::copy(src_path, &dest);

                if let Ok(mut perms) = fs::metadata(&dest).map(|m| m.permissions()) {
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&dest, perms);
                }

                if SonameScanner::is_elf(&dest) {
                    let _ = ElfMutator::mutate_library(&dest, package_id);
                }

                companion_libs.push(dest.to_string_lossy().to_string());
            }
        }

        // 2. Preserve /opt/<pkg> and /usr/share/<pkg> bundle trees
        let opt_src = raw_dir.join("opt").join(package_id);
        if opt_src.exists() && opt_src.is_dir() {
            copy_dir_all(&opt_src, &opt_dir)?;
        }

        let share_src = raw_dir.join("usr/share").join(package_id);
        let dest_pkg_share = share_dir.join(package_id);
        if share_src.exists() && share_src.is_dir() {
            copy_dir_all(&share_src, &dest_pkg_share)?;
        }

        // 3. Process Executables, mutate ELF headers, and graft into bin_dir
        for src_path in &all_files {
            let file_name = src_path.file_name().unwrap_or_default().to_string_lossy();
            let path_str = src_path.to_string_lossy();

            let is_in_bin = path_str.contains("/bin/")
                || path_str.contains("/sbin/")
                || path_str.contains("/opt/")
                || (path_str.contains("/usr/share/") && is_executable(src_path));

            if is_in_bin && !file_name.contains(".so") && !src_path.is_dir() && !file_name.ends_with(".desktop") && !file_name.ends_with(".png") && !file_name.ends_with(".svg") {
                let real_dest_bin = real_bin_dir.join(&*file_name);
                let trampoline_bin = bin_dir.join(&*file_name);

                let _ = fs::copy(src_path, &real_dest_bin);

                // If it's a shell script with hardcoded /usr/bin/, patch it
                if let Ok(content) = fs::read_to_string(&real_dest_bin) {
                    if content.starts_with("#!") {
                        let patched = content.replace("/usr/bin/", &format!("{}/", bin_dir.display()));
                        let _ = fs::write(&real_dest_bin, patched);
                    }
                }

                if let Ok(mut perms) = fs::metadata(&real_dest_bin).map(|m| m.permissions()) {
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&real_dest_bin, perms);
                }

                if SonameScanner::is_elf(&real_dest_bin) {
                    if let Ok(analysis) = ElfMutator::mutate_binary(&real_dest_bin, package_id, Some(&companion_dir)) {
                        if !analysis.missing_libraries.is_empty() {
                            println!(
                                "  {} Note: Binary references {} host libraries: {:?}",
                                "ℹ️".blue(),
                                analysis.missing_libraries.len(),
                                analysis.missing_libraries
                            );
                        }
                    }
                }

                // Generate high-speed environment trampoline
                Self::create_trampoline(
                    &trampoline_bin,
                    &real_dest_bin,
                    &mimic_root,
                )?;

                binary_paths.push(trampoline_bin.to_string_lossy().to_string());

                // If userland root, symlink trampoline to ~/.local/bin
                if let Ok(home) = std::env::var("HOME") {
                    let local_bin = PathBuf::from(home).join(".local/bin");
                    let _ = fs::create_dir_all(&local_bin);
                    let symlink_path = local_bin.join(&*file_name);
                    let _ = fs::remove_file(&symlink_path);
                    let _ = std::os::unix::fs::symlink(&trampoline_bin, &symlink_path);
                }
            } else if file_name.ends_with(".desktop") {
                let dest_desktop = apps_dir.join(&*file_name);
                Self::patch_and_copy_desktop(src_path, &dest_desktop, &bin_dir)?;
                desktop_files.push(dest_desktop.to_string_lossy().to_string());

                // Export to ~/.local/share/applications for instant Noctalia pickup
                if let Ok(home) = std::env::var("HOME") {
                    let user_apps = PathBuf::from(home).join(".local/share/applications");
                    let _ = fs::create_dir_all(&user_apps);
                    let user_desktop = user_apps.join(&*file_name);
                    let _ = fs::copy(&dest_desktop, user_desktop);
                }
            } else if path_str.contains("/icons/") || path_str.contains("/pixmaps/") {
                if let Ok(home) = std::env::var("HOME") {
                    let home_path = PathBuf::from(home);
                    if path_str.contains("/pixmaps/") {
                        let user_pixmaps = home_path.join(".local/share/pixmaps");
                        let _ = fs::create_dir_all(&user_pixmaps);
                        let _ = fs::copy(src_path, user_pixmaps.join(&*file_name));
                    }

                    if path_str.contains("/icons/") {
                        let user_icons = home_path.join(".local/share/icons");
                        let _ = fs::create_dir_all(&user_icons);
                        let _ = fs::copy(src_path, user_icons.join(&*file_name));
                    }
                }
            }
        }

        // Re-generate all existing trampolines in bin_dir to encompass newly added companion pockets
        Self::refresh_all_trampolines(&mimic_root)?;

        Ok(GraftResult {
            binary_paths,
            companion_libs,
            desktop_files,
        })
    }

    fn refresh_all_trampolines(mimic_root: &Path) -> Result<()> {
        let bin_dir = mimic_root.join("bin");
        let real_bin_dir = bin_dir.join(".real");

        if real_bin_dir.exists() {
            if let Ok(entries) = fs::read_dir(&real_bin_dir) {
                for entry in entries.flatten() {
                    let real_bin = entry.path();
                    if real_bin.is_file() {
                        let file_name = real_bin.file_name().unwrap();
                        let trampoline_bin = bin_dir.join(file_name);
                        let _ = Self::create_trampoline(&trampoline_bin, &real_bin, mimic_root);
                    }
                }
            }
        }
        Ok(())
    }

    fn create_trampoline(
        trampoline_path: &Path,
        real_bin: &Path,
        mimic_root: &Path,
    ) -> Result<()> {
        let lib_root = mimic_root.join("lib");
        let share_root = mimic_root.join("share");

        // Collect all companion pockets under /mimic/lib/
        let mut companion_paths = Vec::new();
        if let Ok(entries) = fs::read_dir(&lib_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    companion_paths.push(p.to_string_lossy().to_string());
                }
            }
        }
        companion_paths.push(lib_root.to_string_lossy().to_string());
        let all_libs_str = companion_paths.join(":");

        let script = format!(
            r#"#!/bin/sh
export LD_LIBRARY_PATH="{all_libs}:$LD_LIBRARY_PATH"
export VLC_PLUGIN_PATH="{all_libs}:$VLC_PLUGIN_PATH"
export XDG_DATA_DIRS="{share_root}:$XDG_DATA_DIRS"
export QT_PLUGIN_PATH="{lib_root}/plugins:$QT_PLUGIN_PATH"
exec "{real_bin}" "$@"
"#,
            all_libs = all_libs_str,
            share_root = share_root.display(),
            lib_root = lib_root.display(),
            real_bin = real_bin.display(),
        );

        fs::write(trampoline_path, script)?;
        let mut perms = fs::metadata(trampoline_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(trampoline_path, perms)?;

        Ok(())
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

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn is_executable(path: &Path) -> bool {
    if let Ok(meta) = fs::metadata(path) {
        return meta.permissions().mode() & 0o111 != 0;
    }
    false
}

fn is_writable(path: &Path) -> bool {
    if path.exists() {
        if let Ok(meta) = fs::metadata(path) {
            return !meta.permissions().readonly();
        }
    }
    false
}
