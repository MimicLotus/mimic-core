use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AurRpcResponse<T> {
    pub version: u32,
    pub resultcount: usize,
    #[serde(rename = "type")]
    pub response_type: String,
    pub results: Vec<T>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct AurPackage {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    #[serde(rename = "URLPath")]
    pub url_path: Option<String>,
    #[serde(rename = "Maintainer")]
    pub maintainer: Option<String>,
    #[serde(rename = "Depends", default)]
    pub depends: Vec<String>,
    #[serde(rename = "MakeDepends", default)]
    pub make_depends: Vec<String>,
    #[serde(rename = "OptDepends", default)]
    pub opt_depends: Vec<String>,
    #[serde(rename = "License", default)]
    pub licenses: Vec<String>,
    #[serde(rename = "NumVotes")]
    pub num_votes: Option<u32>,
    #[serde(rename = "Popularity")]
    pub popularity: Option<f64>,
    #[serde(rename = "LastModified")]
    pub last_modified: Option<i64>,
}

pub struct AurClient {
    client: Client,
}

impl AurClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("mimic/4.0.0 (arch-package-engine)")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Search across AUR RPC v5
    pub async fn search(&self, query: &str) -> Result<Vec<AurPackage>> {
        let url = format!("https://aur.archlinux.org/rpc/v5/search/{}", query);
        let resp = self.client.get(&url).send().await
            .context("Failed to contact AUR RPC")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let data: AurRpcResponse<AurPackage> = resp.json().await
            .context("Failed to parse AUR search response")?;

        Ok(data.results)
    }

    /// Query package details for a single AUR package
    pub async fn info(&self, pkg_name: &str) -> Result<Option<AurPackage>> {
        let url = format!("https://aur.archlinux.org/rpc/v5/info/{}", pkg_name);
        let resp = self.client.get(&url).send().await
            .context("Failed to contact AUR RPC")?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let data: AurRpcResponse<AurPackage> = resp.json().await
            .context("Failed to parse AUR info response")?;

        Ok(data.results.into_iter().next())
    }

    /// Query package details for multiple AUR packages in a single batch
    #[allow(dead_code)]
    pub async fn info_multi(&self, pkgs: &[String]) -> Result<Vec<AurPackage>> {
        if pkgs.is_empty() {
            return Ok(Vec::new());
        }

        let mut query_params = String::new();
        for (i, p) in pkgs.iter().enumerate() {
            if i > 0 {
                query_params.push('&');
            }
            query_params.push_str(&format!("arg[]={}", p));
        }

        let url = format!("https://aur.archlinux.org/rpc/v5/info?{}", query_params);
        let resp = self.client.get(&url).send().await
            .context("Failed to contact AUR RPC")?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let data: AurRpcResponse<AurPackage> = resp.json().await
            .context("Failed to parse AUR multi-info response")?;

        Ok(data.results)
    }
}
