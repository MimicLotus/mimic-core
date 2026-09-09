use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;

#[derive(Debug, Clone, Copy)]
pub enum UpstreamGround {
    DebianSid,
    DebianTrixie,
    DebianBookworm,
}

pub struct MirrorResolver {
    client: Client,
}

impl MirrorResolver {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("mimic-os/3.0.0 (autonomous-predator)")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn resolve_deb(&self, pkg_name: &str) -> Result<String> {
        let suites = ["sid", "trixie", "bookworm"];

        for suite in &suites {
            let download_page = format!(
                "https://packages.debian.org/{}/amd64/{}/download",
                suite, pkg_name
            );

            if let Ok(resp) = self.client.get(&download_page).send().await {
                if resp.status().is_success() {
                    if let Ok(body) = resp.text().await {
                        if let Some(deb_url) = extract_deb_link(&body) {
                            return Ok(deb_url);
                        }
                    }
                }
            }
        }

        anyhow::bail!(
            "Could not resolve Debian package '{}' across sid, trixie, or bookworm repositories",
            pkg_name
        );
    }

    pub async fn resolve_soname_package(&self, soname: &str) -> Result<Option<String>> {
        let suites = ["sid", "trixie", "bookworm"];

        for suite in &suites {
            let search_url = format!(
                "https://packages.debian.org/search?searchon=contents&keywords={}&mode=exactfilename&suite={}&arch=amd64",
                soname, suite
            );

            if let Ok(resp) = self.client.get(&search_url).send().await {
                if resp.status().is_success() {
                    if let Ok(body) = resp.text().await {
                        // Look for table entry matching the exact soname and extract package name
                        if let Some(pkg) = extract_package_from_contents_search(&body, soname, suite) {
                            return Ok(Some(pkg));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    pub async fn fetch_to_file_silent(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        let file_name = url.split('/').last().unwrap_or("organ.deb");
        let dest_file = dest.join(file_name);

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

    pub async fn fetch_to_file(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        let file_name = url.split('/').last().unwrap_or("prey.deb");
        let dest_file = dest.join(file_name);

        println!("{} Downloading prey from mirror: {}", "::".cyan().bold(), url.dimmed());

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
}

fn extract_package_from_contents_search(html: &str, soname: &str, suite: &str) -> Option<String> {
    use regex::Regex;
    // HTML structure: <span class="keyword">libavcodec.so.62</span></td> <td> <a href="/sid/libavcodec62">
    let pattern_str = format!(
        r#"class="keyword">{}</span>.*?<a href="/{}/([^"/]+)""#,
        regex::escape(soname),
        suite
    );
    if let Ok(re) = Regex::new(&pattern_str) {
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }
    }

    // Fallback search: any href with /<suite>/<pkg_name> inside a table row containing soname
    for line in html.lines() {
        if line.contains(soname) || line.contains(&format!("/{}", suite)) {
            if let Some(start) = line.find(&format!("/{}", suite)) {
                let rest = &line[start + suite.len() + 2..];
                if let Some(end) = rest.find('"') {
                    let candidate = &rest[..end];
                    if !candidate.is_empty() && !candidate.contains('/') {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
    }

    None
}

fn extract_deb_link(html: &str) -> Option<String> {
    for line in html.lines() {
        if line.contains("http://ftp.debian.org/debian/pool/") || line.contains("http://http.us.debian.org/debian/pool/") {
            for token in line.split('"') {
                if token.starts_with("http://") && token.ends_with(".deb") && token.contains("/pool/") {
                    // Normalize to high speed deb.debian.org CDN
                    let normalized = token
                        .replace("http://ftp.debian.org/", "http://deb.debian.org/")
                        .replace("http://http.us.debian.org/", "http://deb.debian.org/")
                        .replace("http://ftp.us.debian.org/", "http://deb.debian.org/");
                    return Some(normalized);
                }
            }
        }
    }
    None
}
