use anyhow::Result;
use colored::*;

use super::AlpmEngine;

impl AlpmEngine {
    pub fn sync_databases(&mut self, force: bool) -> Result<()> {
        if self.is_sandbox {
            println!(
                "{} Operating in sandbox mode (root: {})",
                "🛡️".cyan().bold(),
                self.config.root_dir.display().to_string().cyan()
            );
        }

        let cpu_tier = crate::config::detect_cpu_tier();
        println!("{} Detected CPU Microarchitecture Tier: {}", "⚡".yellow().bold(), cpu_tier.as_str().bold().green());
        println!("{} Synchronizing ALPM package databases...", "::".cyan().bold());

        for repo in &self.config.repositories {
            println!("  • {} Repository: {}", "::".dimmed(), repo.name.bold().cyan());
        }

        let syncdbs = self.handle.syncdbs_mut();
        if syncdbs.is_empty() {
            println!("  {} No repositories configured. Check /etc/pacman.conf.", "⚠️".yellow());
            return Ok(());
        }

        match syncdbs.update(force) {
            Ok(updated) => {
                if updated {
                    println!("\n{} Synchronization complete: Databases updated successfully.", "✔".green().bold());
                } else {
                    println!("\n{} Synchronization complete: All databases are up to date.", "✔".green().bold());
                }
            }
            Err(e) => {
                eprintln!("\n{} Synchronization encountered an error: {}", "✖".red().bold(), e);
            }
        }

        Ok(())
    }

    /// Return installed packages that do not exist in any registered syncdb
    pub fn get_foreign_packages(&self) -> Vec<(String, String)> {
        let local_db = self.handle.localdb();
        let syncdbs = self.handle.syncdbs();
        let mut foreign = Vec::new();

        for pkg in local_db.pkgs() {
            let name = pkg.name();
            let exists_in_sync = syncdbs.iter().any(|db| db.pkg(name).is_ok());
            if !exists_in_sync {
                foreign.push((name.to_string(), pkg.version().to_string()));
            }
        }
        foreign
    }
}

