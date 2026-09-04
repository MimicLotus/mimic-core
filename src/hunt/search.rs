use anyhow::{Context, Result};
use colored::*;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct DebianSearchResponse {
    results: Option<DebianSearchResults>,
}

#[derive(Debug, Deserialize)]
struct DebianSearchResults {
    exact: Option<DebianSearchItem>,
    other: Option<Vec<DebianSearchItem>>,
}

#[derive(Debug, Deserialize)]
pub struct DebianSearchItem {
    pub package: String,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AurSearchResponse {
    results: Option<Vec<AurSearchItem>>,
}

#[derive(Debug, Deserialize)]
struct AurSearchItem {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArchSearchResponse {
    results: Option<Vec<ArchSearchItem>>,
}

#[derive(Debug, Deserialize)]
struct ArchSearchItem {
    pub pkgname: String,
    pub pkgver: String,
    pub pkgrel: String,
    pub pkgdesc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HuntResult {
    pub origin: String,
    pub package: String,
    pub version: String,
    pub description: String,
}

pub struct Hunter {
    client: Client,
}

impl Hunter {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("mimic-scavenger/3.0.0 (autonomous-predator)")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Search across upstream Debian package repositories
    pub async fn hunt_debian(&self, query: &str) -> Result<Vec<HuntResult>> {
        let url = format!("https://sources.debian.org/api/search/{}/", query);
        let resp = self.client.get(&url).send().await
            .context("Failed to contact Debian API")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let data: DebianSearchResponse = resp.json().await.unwrap_or(DebianSearchResponse { results: None });
        let mut results = Vec::new();

        if let Some(res) = data.results {
            if let Some(exact) = res.exact {
                results.push(HuntResult {
                    origin: "deb".to_string(),
                    package: exact.package,
                    version: exact.version.unwrap_or_else(|| "sid".to_string()),
                    description: "Debian upstream match".to_string(),
                });
            }
            if let Some(others) = res.other {
                for item in others.into_iter().take(8) {
                    results.push(HuntResult {
                        origin: "deb".to_string(),
                        package: item.package,
                        version: item.version.unwrap_or_else(|| "pool".to_string()),
                        description: "Debian pool package".to_string(),
                    });
                }
            }
        }

        Ok(results)
    }

    /// Search across Arch User Repository (AUR)
    pub async fn hunt_aur(&self, query: &str) -> Result<Vec<HuntResult>> {
        let url = format!("https://aur.archlinux.org/rpc/v5/search/{}", query);
        let resp = self.client.get(&url).send().await
            .context("Failed to contact AUR API")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let data: AurSearchResponse = resp.json().await.unwrap_or(AurSearchResponse { results: None });
        let mut results = Vec::new();

        if let Some(items) = data.results {
            for item in items.into_iter().take(8) {
                results.push(HuntResult {
                    origin: "aur".to_string(),
                    package: item.name,
                    version: item.version,
                    description: item.description.unwrap_or_default(),
                });
            }
        }

        Ok(results)
    }

    /// Search across Arch official package repositories
    pub async fn hunt_arch(&self, query: &str) -> Result<Vec<HuntResult>> {
        let url = format!("https://archlinux.org/packages/search/json/?q={}", query);
        let resp = self.client.get(&url).send().await
            .context("Failed to contact Arch API")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let data: ArchSearchResponse = resp.json().await.unwrap_or(ArchSearchResponse { results: None });
        let mut results = Vec::new();

        if let Some(items) = data.results {
            for item in items.into_iter().take(8) {
                results.push(HuntResult {
                    origin: "arch".to_string(),
                    package: item.pkgname,
                    version: format!("{}-{}", item.pkgver, item.pkgrel),
                    description: item.pkgdesc.unwrap_or_default(),
                });
            }
        }

        Ok(results)
    }

    /// Global unified radar hunt across all hunting grounds concurrently
    pub async fn hunt_all(&self, query: &str, filter: Option<&str>) -> Vec<HuntResult> {
        let mut aggregated = Vec::new();

        match filter {
            Some("deb") => {
                if let Ok(res) = self.hunt_debian(query).await {
                    aggregated.extend(res);
                }
            }
            Some("aur") => {
                if let Ok(res) = self.hunt_aur(query).await {
                    aggregated.extend(res);
                }
            }
            Some("arch") => {
                if let Ok(res) = self.hunt_arch(query).await {
                    aggregated.extend(res);
                }
            }
            _ => {
                // Concurrent multi-ecosystem query
                let (deb_res, aur_res, arch_res) = tokio::join!(
                    self.hunt_debian(query),
                    self.hunt_aur(query),
                    self.hunt_arch(query)
                );

                if let Ok(res) = deb_res {
                    aggregated.extend(res);
                }
                if let Ok(res) = arch_res {
                    aggregated.extend(res);
                }
                if let Ok(res) = aur_res {
                    aggregated.extend(res);
                }
            }
        }

        aggregated
    }
}
