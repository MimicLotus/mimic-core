pub mod fetch;
pub mod git_source;
pub mod memory_guard;
pub mod psi;
pub mod sandbox;

#[allow(unused_imports)]
pub use git_source::GitSourceRunner;
#[allow(unused_imports)]
pub use memory_guard::{MemoryGuard, MemoryProfile};
#[allow(unused_imports)]
pub use psi::PsiWatcher;

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use colored::*;

use fetch::AurFetcher;
use sandbox::BubblewrapSandbox;

pub struct AurBuilder {
    fetcher: AurFetcher,
}

impl AurBuilder {
    pub fn new() -> Self {
        Self {
            fetcher: AurFetcher::new(),
        }
    }

    /// Build an AUR package inside a hermetic bwrap container.
    /// Returns the paths to generated .pkg.tar.zst package artifacts.
    pub async fn build(&self, pkg_name: &str, output_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
        // NVMe workspace: relocated from tmpfs to prevent memory exhaustion
        let base_build_dir = PathBuf::from("/var/tmp/mimic-build").join(pkg_name);
        if base_build_dir.exists() {
            let _ = std::fs::remove_dir_all(&base_build_dir);
        }
        std::fs::create_dir_all(&base_build_dir)
            .with_context(|| format!("Failed to create build workspace at {:?}", base_build_dir))?;

        println!("{} Fetching AUR snapshot for '{}'...", "::".cyan().bold(), pkg_name.bold());
        let pkg_dir = self.fetcher.fetch_and_unpack(pkg_name, &base_build_dir).await?;

        println!("{} PKGBUILD unpacked into {:?}", "✔".green().bold(), pkg_dir);

        // Execute sandboxed build
        let sandbox = BubblewrapSandbox::new(pkg_name, &pkg_dir);
        sandbox.execute_build()?;

        // Scan for generated packages (*.pkg.tar.zst or *.pkg.tar.xz)
        let mut built_packages = Vec::new();
        for entry in std::fs::read_dir(&pkg_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if (file_name.ends_with(".pkg.tar.zst") || file_name.ends_with(".pkg.tar.xz") || file_name.ends_with(".pkg.tar.gz"))
                        && !file_name.ends_with(".sig")
                    {
                        if let Some(dest_dir) = output_dir {
                            let _ = std::fs::create_dir_all(dest_dir);
                            let dest_path = dest_dir.join(file_name);
                            std::fs::copy(&path, &dest_path)?;
                            built_packages.push(dest_path);
                        } else {
                            built_packages.push(path);
                        }
                    }
                }
            }
        }

        if built_packages.is_empty() {
            anyhow::bail!("Compilation completed, but no .pkg.tar.zst artifacts were found in {:?}", pkg_dir);
        }

        println!("\n{} Generated package artifacts:", "::".green().bold());
        for pkg in &built_packages {
            println!("  📦 {}", pkg.display().to_string().bold().green());
        }

        Ok(built_packages)
    }
}
