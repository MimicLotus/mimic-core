use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use reqwest::Client;
use tar::Archive;

pub struct AurFetcher {
    client: Client,
}

impl AurFetcher {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("mimic/4.0.0 (arch-package-engine)")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Download AUR snapshot tarball and extract into destination directory
    pub async fn fetch_and_unpack(&self, pkg_name: &str, dest_dir: &Path) -> Result<PathBuf> {
        let url = format!("https://aur.archlinux.org/cgit/aur.git/snapshot/{}.tar.gz", pkg_name);
        let resp = self.client.get(&url).send().await
            .with_context(|| format!("Failed to download AUR snapshot for '{}'", pkg_name))?;

        if !resp.status().is_success() {
            anyhow::bail!("AUR snapshot download failed with HTTP {}", resp.status());
        }

        let bytes = resp.bytes().await
            .context("Failed to read AUR snapshot response bytes")?;

        let tar = GzDecoder::new(&bytes[..]);
        let mut archive = Archive::new(tar);

        std::fs::create_dir_all(dest_dir)
            .with_context(|| format!("Failed to create build directory at {:?}", dest_dir))?;

        archive.unpack(dest_dir)
            .with_context(|| format!("Failed to unpack AUR archive into {:?}", dest_dir))?;

        let pkg_dir = dest_dir.join(pkg_name);
        if pkg_dir.exists() && pkg_dir.join("PKGBUILD").exists() {
            Ok(pkg_dir)
        } else if dest_dir.join("PKGBUILD").exists() {
            Ok(dest_dir.to_path_buf())
        } else {
            anyhow::bail!("PKGBUILD not found in extracted snapshot at {:?}", dest_dir);
        }
    }
}
