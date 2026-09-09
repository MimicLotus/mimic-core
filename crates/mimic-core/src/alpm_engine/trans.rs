use anyhow::{bail, Context, Result};
use colored::*;

use super::AlpmEngine;

#[derive(Debug, Clone)]
pub struct TransactionPlan {
    pub to_add: Vec<(String, String, i64)>,      // (name, version, download_size)
    pub to_remove: Vec<(String, String)>,       // (name, version)
    pub total_download_size: i64,
    pub net_installed_size: i64,
}

impl AlpmEngine {
    pub fn plan_install(&mut self, targets: &[String], sysupgrade: bool) -> Result<TransactionPlan> {
        if self.is_sandbox {
            println!(
                "{} Operating in sandbox mode (root: {})",
                "🛡️".cyan().bold(),
                self.config.root_dir.display().to_string().cyan()
            );
        }

        let flags = alpm::TransFlag::NONE;
        self.handle.trans_init(flags)
            .context("Failed to initialize ALPM transaction")?;

        let mut plan = TransactionPlan {
            to_add: Vec::new(),
            to_remove: Vec::new(),
            total_download_size: 0,
            net_installed_size: 0,
        };

        // 1. If sysupgrade requested, flag sysupgrade
        if sysupgrade {
            let upgrade_res = self.handle.sync_sysupgrade(false).map_err(|e| e.to_string());
            if let Err(err_msg) = upgrade_res {
                let _ = self.handle.trans_release();
                bail!("Failed to plan sysupgrade: {}", err_msg);
            }
        }

        // 2. Add individual targets
        for target in targets {
            let target_path = std::path::Path::new(target);
            if (target.ends_with(".pkg.tar.zst") || target.ends_with(".pkg.tar.xz") || target.ends_with(".pkg.tar.gz"))
                && target_path.exists()
            {
                let add_res = match self.handle.pkg_load(target.as_str(), true, alpm::SigLevel::NONE) {
                    Ok(pkg) => self.handle.trans_add_pkg(pkg).map_err(|e| e.to_string()),
                    Err(e) => Err(format!("Failed to load local package archive: {}", e)),
                };

                if let Err(err_msg) = add_res {
                    let _ = self.handle.trans_release();
                    bail!("Failed to add local package '{}' to transaction: {}", target, err_msg);
                }
                continue;
            }

            let mut found_pkg = None;

            for db in self.handle.syncdbs() {
                if let Ok(pkg) = db.pkg(target.as_str()) {
                    found_pkg = Some(pkg);
                    break;
                }
            }

            match found_pkg {
                Some(pkg) => {
                    let add_res = self.handle.trans_add_pkg(pkg).map_err(|e| e.to_string());
                    if let Err(err_msg) = add_res {
                        let _ = self.handle.trans_release();
                        bail!("Failed to add package '{}' to transaction: {}", target, err_msg);
                    }
                }
                None => {
                    let _ = self.handle.trans_release();
                    bail!("Target package '{}' not found in any registered repository.", target);
                }
            }
        }

        // 3. Prepare transaction (resolves dependencies and conflicts)
        let prepare_res = self.handle.trans_prepare().map_err(|e| e.to_string());
        if let Err(err_msg) = prepare_res {
            let _ = self.handle.trans_release();
            bail!("Transaction preparation failed (dependency/conflict error): {}", err_msg);
        }

        // 4. Inspect transaction targets
        for pkg in self.handle.trans_add() {
            plan.to_add.push((pkg.name().to_string(), pkg.version().to_string(), pkg.size()));
            plan.total_download_size += pkg.size();
            plan.net_installed_size += pkg.isize();
        }

        for pkg in self.handle.trans_remove() {
            plan.to_remove.push((pkg.name().to_string(), pkg.version().to_string()));
            plan.net_installed_size -= pkg.isize();
        }

        Ok(plan)
    }

    pub fn commit_transaction(&mut self) -> Result<()> {
        println!("{} Committing transaction to disk...", "::".cyan().bold());

        let commit_res = self.handle.trans_commit();
        let _ = self.handle.trans_release();

        if let Err(e) = commit_res {
            bail!("Transaction commit failed: {}", e);
        }

        println!("{} Transaction successfully committed.", "✔".green().bold());
        Ok(())
    }

    pub fn release_transaction(&mut self) {
        let _ = self.handle.trans_release();
    }

    pub fn plan_remove(&mut self, targets: &[String], cascade: bool) -> Result<TransactionPlan> {
        let mut flags = alpm::TransFlag::NONE;
        if cascade {
            flags |= alpm::TransFlag::CASCADE;
            flags |= alpm::TransFlag::RECURSE;
        }

        self.handle.trans_init(flags)
            .context("Failed to initialize ALPM remove transaction")?;

        let mut plan = TransactionPlan {
            to_add: Vec::new(),
            to_remove: Vec::new(),
            total_download_size: 0,
            net_installed_size: 0,
        };

        let local_db = self.handle.localdb();

        for target in targets {
            match local_db.pkg(target.as_str()) {
                Ok(pkg) => {
                    let rem_res = self.handle.trans_remove_pkg(pkg).map_err(|e| e.to_string());
                    if let Err(err_msg) = rem_res {
                        let _ = self.handle.trans_release();
                        bail!("Failed to add package '{}' to removal list: {}", target, err_msg);
                    }
                }
                Err(_) => {
                    let _ = self.handle.trans_release();
                    bail!("Package '{}' is not currently installed.", target);
                }
            }
        }

        let prepare_res = self.handle.trans_prepare().map_err(|e| e.to_string());
        if let Err(err_msg) = prepare_res {
            let _ = self.handle.trans_release();
            bail!("Remove transaction preparation failed: {}", err_msg);
        }

        for pkg in self.handle.trans_remove() {
            plan.to_remove.push((pkg.name().to_string(), pkg.version().to_string()));
            plan.net_installed_size -= pkg.isize();
        }

        Ok(plan)
    }

    pub fn print_plan(&self, plan: &TransactionPlan) {
        if !plan.to_add.is_empty() {
            println!("\n  {} Packages to Install/Upgrade ({}):", "📦".cyan(), plan.to_add.len());
            for (name, ver, size) in &plan.to_add {
                let size_str = format_bytes(*size as u64);
                println!("    • {} {} ({})", name.bold().white(), ver.cyan(), size_str.dimmed());
            }
        }

        if !plan.to_remove.is_empty() {
            println!("\n  {} Packages to Remove ({}):", "🗑️".red(), plan.to_remove.len());
            for (name, ver) in &plan.to_remove {
                println!("    • {} {}", name.bold().red(), ver.dimmed());
            }
        }

        println!();
        if plan.total_download_size > 0 {
            println!("  • Total Download Size:  {}", format_bytes(plan.total_download_size as u64).bold().cyan());
        }
        if plan.net_installed_size > 0 {
            println!("  • Net Installed Size:  {}", format_bytes(plan.net_installed_size as u64).bold().green());
        } else if plan.net_installed_size < 0 {
            println!("  • Space to be Freed:   {}", format_bytes((-plan.net_installed_size) as u64).bold().red());
        }
        println!();
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
