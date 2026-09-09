use colored::*;
use super::AlpmEngine;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub repo: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub installed_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PackageDetails {
    pub repo: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub licenses: Vec<String>,
    pub dependencies: Vec<String>,
    pub optional_deps: Vec<String>,
    pub packager: String,
    pub download_size: i64,
    pub installed_size: i64,
    pub build_date: String,
    pub is_installed: bool,
}

impl AlpmEngine {
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        let local_db = self.handle.localdb();

        for db in self.handle.syncdbs() {
            let repo_name = db.name().to_string();

            for pkg in db.pkgs() {
                let pkg_name = pkg.name();
                let pkg_desc = pkg.desc().unwrap_or("");

                if pkg_name.to_lowercase().contains(&query_lower) || pkg_desc.to_lowercase().contains(&query_lower) {
                    let installed_version = local_db.pkg(pkg_name).ok().map(|p| p.version().to_string());

                    results.push(SearchResult {
                        repo: repo_name.clone(),
                        name: pkg_name.to_string(),
                        version: pkg.version().to_string(),
                        description: pkg_desc.to_string(),
                        installed_version,
                    });
                }
            }
        }

        results
    }

    pub fn info(&self, target: &str) -> Option<PackageDetails> {
        let local_db = self.handle.localdb();
        let is_installed = local_db.pkg(target).is_ok();

        // Check localdb first, then syncdbs
        if let Ok(pkg) = local_db.pkg(target) {
            return Some(PackageDetails {
                repo: "local".to_string(),
                name: pkg.name().to_string(),
                version: pkg.version().to_string(),
                description: pkg.desc().unwrap_or("").to_string(),
                url: pkg.url().unwrap_or("").to_string(),
                licenses: pkg.licenses().into_iter().map(|s| s.to_string()).collect(),
                dependencies: pkg.depends().into_iter().map(|d| d.name().to_string()).collect(),
                optional_deps: pkg.optdepends().into_iter().map(|d| d.name().to_string()).collect(),
                packager: pkg.packager().unwrap_or("").to_string(),
                download_size: 0,
                installed_size: pkg.isize(),
                build_date: pkg.build_date().to_string(),
                is_installed: true,
            });
        }

        for db in self.handle.syncdbs() {
            if let Ok(pkg) = db.pkg(target) {
                return Some(PackageDetails {
                    repo: db.name().to_string(),
                    name: pkg.name().to_string(),
                    version: pkg.version().to_string(),
                    description: pkg.desc().unwrap_or("").to_string(),
                    url: pkg.url().unwrap_or("").to_string(),
                    licenses: pkg.licenses().into_iter().map(|s| s.to_string()).collect(),
                    dependencies: pkg.depends().into_iter().map(|d| d.name().to_string()).collect(),
                    optional_deps: pkg.optdepends().into_iter().map(|d| d.name().to_string()).collect(),
                    packager: pkg.packager().unwrap_or("").to_string(),
                    download_size: pkg.size(),
                    installed_size: pkg.isize(),
                    build_date: pkg.build_date().to_string(),
                    is_installed,
                });
            }
        }

        None
    }

    pub fn print_search_results(&self, query: &str, results: &[SearchResult]) {
        if results.is_empty() {
            println!("  {}", format!("No packages found matching '{}'.", query).yellow());
            return;
        }

        for r in results {
            let repo_badge = match r.repo.as_str() {
                "core" => "core".bold().red(),
                "extra" => "extra".bold().cyan(),
                "multilib" => "multilib".bold().magenta(),
                "cachyos" | "cachyos-v3" | "cachyos-v4" => r.repo.bold().yellow(),
                other => other.bold().blue(),
            };

            let installed_badge = if let Some(ref ver) = r.installed_version {
                format!(" [installed: {}]", ver).green().bold()
            } else {
                "".normal()
            };

            println!(
                "{}/{} {} {}{}",
                repo_badge,
                r.name.bold().white(),
                r.version.green(),
                format!("({})", r.repo).dimmed(),
                installed_badge
            );

            if !r.description.is_empty() {
                println!("    {}", r.description.dimmed());
            }
        }
    }
}
