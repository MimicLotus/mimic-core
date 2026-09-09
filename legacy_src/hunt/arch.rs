use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use tar::Archive as TarArchive;
use zstd::stream::read::Decoder as ZstdDecoder;

#[derive(Debug, Clone)]
pub struct ArchPackageInfo {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub extracted_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ArchSearchResponse {
    results: Vec<ArchSearchResult>,
}

#[derive(Debug, Deserialize)]
struct ArchSearchResult {
    pkgname: String,
    pkgver: String,
    pkgrel: String,
    repo: String,
    arch: String,
    pkgdesc: Option<String>,
    filename: Option<String>,
}

pub struct ArchHunter {
    client: Client,
}

impl ArchHunter {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("mimic-os/3.0.0 (autonomous-predator)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn resolve_arch_pkg(&self, raw_name: &str) -> Result<(String, String)> {
        let pkg_name = raw_name
            .split('>')
            .next()
            .unwrap_or("")
            .split('<')
            .next()
            .unwrap_or("")
            .split('=')
            .next()
            .unwrap_or("")
            .trim();

        if pkg_name.is_empty() {
            anyhow::bail!("Invalid empty package name");
        }

        // Query Arch search JSON API
        let search_url = format!("https://archlinux.org/packages/search/json/?name={}", pkg_name);

        if let Ok(resp) = self.client.get(&search_url).send().await {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<ArchSearchResponse>().await {
                    for r in data.results {
                        if r.pkgname == pkg_name && (r.arch == "x86_64" || r.arch == "any") {
                            let ver = format!("{}-{}", r.pkgver, r.pkgrel);
                            let download_url = format!(
                                "https://archlinux.org/packages/{}/{}/{}/download/",
                                r.repo, r.arch, r.pkgname
                            );
                            return Ok((download_url, ver));
                        }
                    }
                }
            }
        }

        // Direct fallback across standard repos: extra, core, multilib
        for repo in &["extra", "core", "multilib"] {
            let download_url = format!(
                "https://archlinux.org/packages/{}/x86_64/{}/download/",
                repo, pkg_name
            );
            if let Ok(resp) = self.client.head(&download_url).send().await {
                if resp.status().is_success() || resp.status().is_redirection() {
                    return Ok((download_url, "latest".to_string()));
                }
            }
        }

        anyhow::bail!("Could not resolve Arch package '{}' across extra, core, or multilib", pkg_name);
    }

    pub async fn resolve_soname_package(&self, soname: &str) -> Result<Option<String>> {
        let soname_lower = soname.to_lowercase();
        let parts: Vec<&str> = soname_lower.split(".so").collect();
        let base_name = parts[0];

        // 1. KF6 mappings e.g. "libkf6archive" -> "karchive"
        if let Some(kf_name) = base_name.strip_prefix("libkf6") {
            let arch_name = format!("k{}", kf_name);
            if let Ok((_, _)) = self.resolve_arch_pkg(&arch_name).await {
                return Ok(Some(arch_name));
            }
        }

        // 2. Qt6 mappings e.g. "libqt6svg" -> "qt6-svg"
        if let Some(qt_name) = base_name.strip_prefix("libqt6") {
            let arch_name = format!("qt6-{}", qt_name);
            if let Ok((_, _)) = self.resolve_arch_pkg(&arch_name).await {
                return Ok(Some(arch_name));
            }
        }

        // 3. Try base_name without "lib" prefix e.g. "libmlt-7" -> "mlt", "libopentimelineio" -> "opentimelineio"
        if let Some(without_lib) = base_name.strip_prefix("lib") {
            let clean_no_ver = without_lib.split('-').next().unwrap_or(without_lib);
            if let Ok((_, _)) = self.resolve_arch_pkg(clean_no_ver).await {
                return Ok(Some(clean_no_ver.to_string()));
            }
            if let Ok((_, _)) = self.resolve_arch_pkg(without_lib).await {
                return Ok(Some(without_lib.to_string()));
            }
        }

        // 4. Try exact base_name e.g. "libpng", "libjpeg-turbo"
        if let Ok((_, _)) = self.resolve_arch_pkg(base_name).await {
            return Ok(Some(base_name.to_string()));
        }

        // 5. Query search API for packages matching base_name
        let search_url = format!("https://archlinux.org/packages/search/json/?q={}", base_name);
        if let Ok(resp) = self.client.get(&search_url).send().await {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<ArchSearchResponse>().await {
                    if let Some(first) = data.results.into_iter().next() {
                        return Ok(Some(first.pkgname));
                    }
                }
            }
        }

        Ok(None)
    }

    pub async fn fetch_to_file(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        let file_name = url.split('/').filter(|s| !s.is_empty()).last().unwrap_or("pkg.tar.zst");
        let safe_name = if file_name.ends_with(".pkg.tar.zst") {
            file_name.to_string()
        } else {
            format!("{}.pkg.tar.zst", file_name)
        };
        let dest_file = dest.join(safe_name);

        println!("{} Downloading prey from Arch mirrors: {}", "::".cyan().bold(), url.dimmed());

        let resp = self.client.get(url).send().await
            .with_context(|| format!("Failed to connect to mirror at {}", url))?;

        if !resp.status().is_success() {
            anyhow::bail!("Mirror returned HTTP error: {}", resp.status());
        }

        let total_size = resp.content_length().unwrap_or(0);
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("#>-"),
        );

        let mut file = File::create(&dest_file)
            .with_context(|| format!("Failed to create destination file at {:?}", dest_file))?;
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.with_context(|| "Error reading response stream")?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }

        pb.finish_with_message("Download complete");
        Ok(dest_file)
    }

    pub async fn fetch_to_file_silent(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        let file_name = url.split('/').filter(|s| !s.is_empty()).last().unwrap_or("organ.pkg.tar.zst");
        let safe_name = if file_name.ends_with(".pkg.tar.zst") {
            file_name.to_string()
        } else {
            format!("{}.pkg.tar.zst", file_name)
        };
        let dest_file = dest.join(safe_name);

        let resp = self.client.get(url).send().await
            .with_context(|| format!("Failed to connect to mirror at {}", url))?;

        if !resp.status().is_success() {
            anyhow::bail!("Mirror returned HTTP error: {}", resp.status());
        }

        let mut file = File::create(&dest_file)
            .with_context(|| format!("Failed to create destination file at {:?}", dest_file))?;
        let mut stream = resp.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.with_context(|| "Error reading response stream")?;
            file.write_all(&chunk)?;
        }

        Ok(dest_file)
    }

    pub fn extract_arch_pkg(pkg_path: &Path, destination: &Path) -> Result<ArchPackageInfo> {
        let raw_extract_dir = destination.join("raw");
        std::fs::create_dir_all(&raw_extract_dir)?;
        Self::extract_arch_pkg_into(pkg_path, &raw_extract_dir)
    }

    pub fn extract_arch_pkg_into(pkg_path: &Path, raw_extract_dir: &Path) -> Result<ArchPackageInfo> {
        let file = File::open(pkg_path)
            .with_context(|| format!("Failed to open Arch package at {:?}", pkg_path))?;
        
        let decoder = ZstdDecoder::new(file)
            .with_context(|| format!("Failed to create zstd decoder for {:?}", pkg_path))?;
        let mut tar = TarArchive::new(decoder);
        tar.unpack(raw_extract_dir)
            .with_context(|| format!("Failed to unpack tar payload from {:?}", pkg_path))?;

        // Parse .PKGINFO if present
        let pkginfo_path = raw_extract_dir.join(".PKGINFO");
        let mut pkg_name = String::new();
        let mut version = String::new();
        let mut architecture = "x86_64".to_string();
        let mut description = String::new();
        let mut dependencies = Vec::new();

        if pkginfo_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&pkginfo_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(val) = trimmed.strip_prefix("pkgname = ") {
                        pkg_name = val.trim().to_string();
                    } else if let Some(val) = trimmed.strip_prefix("pkgver = ") {
                        version = val.trim().to_string();
                    } else if let Some(val) = trimmed.strip_prefix("arch = ") {
                        architecture = val.trim().to_string();
                    } else if let Some(val) = trimmed.strip_prefix("pkgdesc = ") {
                        description = val.trim().to_string();
                    } else if let Some(val) = trimmed.strip_prefix("depend = ") {
                        let dep_token = val.trim();
                        // Strip version constraint e.g. "ffmpeg>=7.0" -> "ffmpeg"
                        let clean_dep = dep_token
                            .split('>')
                            .next()
                            .unwrap_or("")
                            .split('<')
                            .next()
                            .unwrap_or("")
                            .split('=')
                            .next()
                            .unwrap_or("")
                            .trim();
                        if !clean_dep.is_empty() {
                            dependencies.push(clean_dep.to_string());
                        }
                    }
                }
            }
        }

        if pkg_name.is_empty() {
            let stem = pkg_path.file_stem().unwrap_or_default().to_string_lossy();
            let clean_stem = stem.trim_end_matches(".pkg.tar");
            let parts: Vec<&str> = clean_stem.split('-').collect();
            pkg_name = parts.first().unwrap_or(&"unknown").to_string();
            version = parts.get(1).unwrap_or(&"1.0.0").to_string();
        }

        Ok(ArchPackageInfo {
            name: pkg_name,
            version,
            architecture,
            description,
            dependencies,
            extracted_dir: raw_extract_dir.to_path_buf(),
        })
    }
}
