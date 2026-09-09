use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Clone)]
pub struct MicroRepoPackage {
    pub repo_name: String,
    pub name: String,
    pub version: String,
    pub arch: String,
    pub download_url: String,
    pub size_bytes: u64,
}

pub struct MicroRepoResolver {
    client: Client,
}

impl MicroRepoResolver {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("mimic/4.0.0 (arch-package-engine)")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Query GitHub Releases API for a repository e.g. "owner/repo"
    pub async fn fetch_release_packages(&self, owner_repo: &str) -> Result<Vec<MicroRepoPackage>> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", owner_repo);
        let resp = self.client.get(&url).send().await
            .with_context(|| format!("Failed to fetch micro-repo release from '{}'", owner_repo))?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let release: GitHubRelease = resp.json().await
            .with_context(|| format!("Failed to parse release metadata for '{}'", owner_repo))?;

        let mut packages = Vec::new();
        let repo_tag = owner_repo.split('/').last().unwrap_or(owner_repo);

        for asset in release.assets {
            if asset.name.ends_with(".pkg.tar.zst") || asset.name.ends_with(".pkg.tar.xz") {
                if let Some((pkgname, version, arch)) = parse_pkg_filename(&asset.name) {
                    packages.push(MicroRepoPackage {
                        repo_name: repo_tag.to_string(),
                        name: pkgname,
                        version,
                        arch,
                        download_url: asset.browser_download_url,
                        size_bytes: asset.size,
                    });
                }
            }
        }

        Ok(packages)
    }

    /// Match target package against configured micro-repositories
    pub async fn find_package(&self, micro_repos: &[String], target_pkg: &str) -> Option<MicroRepoPackage> {
        for repo_slug in micro_repos {
            if let Ok(pkgs) = self.fetch_release_packages(repo_slug).await {
                for p in pkgs {
                    if p.name == target_pkg {
                        return Some(p);
                    }
                }
            }
        }
        None
    }
}

pub fn parse_pkg_filename(filename: &str) -> Option<(String, String, String)> {
    let clean = filename.trim_end_matches(".pkg.tar.zst").trim_end_matches(".pkg.tar.xz");
    let parts: Vec<&str> = clean.rsplitn(3, '-').collect();
    if parts.len() == 3 {
        let arch = parts[0].to_string();
        let rel_ver = parts[1];
        let name_and_base = parts[2];
        let name_parts: Vec<&str> = name_and_base.rsplitn(2, '-').collect();
        if name_parts.len() == 2 {
            let pkgver = format!("{}-{}", name_parts[0], rel_ver);
            let pkgname = name_parts[1].to_string();
            return Some((pkgname, pkgver, arch));
        } else {
            let version = rel_ver.to_string();
            let pkgname = name_and_base.to_string();
            return Some((pkgname, version, arch));
        }
    }
    None
}
