use std::collections::HashSet;
use std::path::Path;
use anyhow::Result;
use colored::*;

use crate::graft::GraftEngine;
use crate::hunt::deb::DebHunter;
use crate::hunt::mirror::MirrorResolver;
use crate::mutator::soname::SonameScanner;

pub struct OrganScavenger;

impl OrganScavenger {
    pub async fn scavenge_dependencies(
        primary_pkg: &str,
        raw_extract_dir: &Path,
        initial_deps: &[String],
        forge_dir: &Path,
    ) -> Result<Vec<String>> {
        let resolver = MirrorResolver::new();
        let mimic_root = GraftEngine::get_mimic_root();
        let companion_root = mimic_root.join("lib");

        let mut companion_dirs = Vec::new();
        if companion_root.exists() {
            if let Ok(entries) = std::fs::read_dir(&companion_root) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        companion_dirs.push(p);
                    }
                }
            }
        }

        let mut fetched_packages = HashSet::new();
        fetched_packages.insert(primary_pkg.to_string());

        let mut candidate_pool = initial_deps.to_vec();
        let mut scavenged_packages = Vec::new();

        let max_iterations = 12;
        let mut iteration = 0;

        println!("{} Phase 1.5: Autonomous Dependency & Organ Scavenging...", "::".cyan().bold());

        loop {
            iteration += 1;
            if iteration > max_iterations {
                println!("  {} Max dependency recursion depth reached.", "⚠️".yellow());
                break;
            }

            // 1. Scan the extracted files for missing SONAMEs
            let missing_sonames = SonameScanner::scan_unresolved_dependencies(raw_extract_dir, &companion_dirs)?;

            if missing_sonames.is_empty() {
                println!("  {} All required dynamic organs & linkages satisfied!", "✔".green().bold());
                break;
            }

            println!(
                "  {} Detected {} missing foreign libraries. Resolving organ donors...",
                "🔍".cyan(),
                missing_sonames.len().to_string().bold().yellow()
            );

            let mut packages_to_fetch = Vec::new();

            for soname in &missing_sonames {
                let mut resolved_pkg = None;

                // Priority 1: Match against current Depends candidate pool
                for cand in &candidate_pool {
                    if !fetched_packages.contains(cand) && soname_matches_package(soname, cand) {
                        resolved_pkg = Some(cand.clone());
                        break;
                    }
                }

                // Priority 2: Online Debian Contents Database Lookup
                if resolved_pkg.is_none() {
                    if let Ok(Some(online_pkg)) = resolver.resolve_soname_package(soname).await {
                        if !fetched_packages.contains(&online_pkg) {
                            resolved_pkg = Some(online_pkg);
                        }
                    }
                }

                if let Some(pkg) = resolved_pkg {
                    if !fetched_packages.contains(&pkg) && !packages_to_fetch.contains(&pkg) {
                        packages_to_fetch.push(pkg);
                    }
                }
            }

            // Fallback: If some SONAMEs couldn't be resolved, check if there are remaining lib-* candidates in pool
            if packages_to_fetch.is_empty() {
                for cand in &candidate_pool {
                    if !fetched_packages.contains(cand) && cand.starts_with("lib") && !is_known_host_base_pkg(cand) {
                        packages_to_fetch.push(cand.clone());
                        if packages_to_fetch.len() >= 6 {
                            break;
                        }
                    }
                }
            }

            if packages_to_fetch.is_empty() {
                println!(
                    "  {} Could not locate upstream donors for {} remaining libraries: {:?}",
                    "⚠️".yellow(),
                    missing_sonames.len(),
                    missing_sonames
                );
                break;
            }

            // Fetch and unpack newly identified dependency packages
            let mut newly_scavenged = 0;
            for dep_pkg in packages_to_fetch {
                fetched_packages.insert(dep_pkg.clone());
                println!("  • {} Scavenging companion organ: '{}'...", "🧬".green(), dep_pkg.bold().green());

                match resolver.resolve_deb(&dep_pkg).await {
                    Ok(dep_url) => {
                        match resolver.fetch_to_file_silent(&dep_url, forge_dir).await {
                            Ok(dep_deb_path) => {
                                match DebHunter::extract_deb_into(&dep_deb_path, raw_extract_dir) {
                                    Ok(dep_info) => {
                                        newly_scavenged += 1;
                                        scavenged_packages.push(dep_pkg);
                                        for nd in dep_info.dependencies {
                                            if !candidate_pool.contains(&nd) {
                                                candidate_pool.push(nd);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!("    {} Failed to extract organ {}: {}", "⚠️".yellow(), dep_pkg, e);
                                    }
                                }
                            }
                            Err(e) => {
                                println!("    {} Failed to download organ {}: {}", "⚠️".yellow(), dep_pkg, e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("    {} Upstream mirror resolution failed for {}: {}", "⚠️".yellow(), dep_pkg, e);
                    }
                }
            }

            if newly_scavenged == 0 {
                break;
            }
        }

        Ok(scavenged_packages)
    }
}

fn soname_matches_package(soname: &str, pkg: &str) -> bool {
    let soname_lower = soname.to_lowercase();
    let pkg_lower = pkg.to_lowercase();

    // Strip .so.* suffix from soname e.g. "libavcodec.so.62" -> "libavcodec", "62"
    let parts: Vec<&str> = soname_lower.split(".so").collect();
    let base_name = parts[0];
    let so_ver = parts.get(1).map(|s| s.trim_start_matches('.')).unwrap_or("");

    // 1. Exact match e.g. "libavcodec62"
    if pkg_lower == format!("{}{}", base_name, so_ver) {
        return true;
    }

    // 2. Prefix with version e.g. "libcdio19t64"
    if !so_ver.is_empty() && pkg_lower.starts_with(&format!("{}{}", base_name, so_ver)) {
        return true;
    }

    // 3. Prefix match with dash or contains e.g. "libjpeg62-turbo", "liblua5.2-0"
    if pkg_lower.starts_with(base_name) && (!so_ver.is_empty() && pkg_lower.contains(so_ver)) {
        return true;
    }

    // 4. Dot versions e.g. "libcodec2.so.1.2" vs "libcodec2-1.2"
    if so_ver.contains('.') {
        let dash_ver = so_ver.replace('.', "-");
        if pkg_lower.contains(&dash_ver) || pkg_lower.contains(so_ver) {
            return true;
        }
    }

    // 5. General prefix match e.g. "libplacebo.so.360" vs "libplacebo360"
    let clean_so = soname_lower.replace(".so.", "").replace(".so", "");
    if pkg_lower.starts_with(&clean_so) || clean_so.starts_with(&pkg_lower) {
        return true;
    }

    false
}

fn is_known_host_base_pkg(pkg: &str) -> bool {
    let base_pkgs = [
        "libc6",
        "libc-bin",
        "libgcc-s1",
        "libstdc++6",
        "linux-libc-dev",
        "base-files",
        "base-passwd",
        "dpkg",
        "debconf",
    ];
    base_pkgs.contains(&pkg)
}
