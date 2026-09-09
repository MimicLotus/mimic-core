use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuTier {
    V4,
    V3,
    Generic,
}

impl CpuTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            CpuTier::V4 => "v4",
            CpuTier::V3 => "v3",
            CpuTier::Generic => "generic",
        }
    }
}

pub fn detect_cpu_tier() -> CpuTier {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("avx512cd")
            && is_x86_feature_detected!("avx512dq")
            && is_x86_feature_detected!("avx512vl")
        {
            return CpuTier::V4;
        }

        if is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("fma")
            && is_x86_feature_detected!("bmi1")
            && is_x86_feature_detected!("bmi2")
        {
            return CpuTier::V3;
        }
    }

    CpuTier::Generic
}

#[derive(Debug, Clone)]
pub struct Repository {
    pub name: String,
    pub servers: Vec<String>,
    pub sig_level: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PacmanConfig {
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub cache_dirs: Vec<PathBuf>,
    pub hook_dirs: Vec<PathBuf>,
    pub gpg_dir: PathBuf,
    pub log_file: Option<PathBuf>,
    pub architecture: String,
    pub check_space: bool,
    pub color: bool,
    pub repositories: Vec<Repository>,
}

impl Default for PacmanConfig {
    fn default() -> Self {
        Self {
            root_dir: PathBuf::from("/"),
            db_path: PathBuf::from("/var/lib/pacman"),
            cache_dirs: vec![PathBuf::from("/var/cache/pacman/pkg")],
            hook_dirs: vec![PathBuf::from("/etc/pacman.d/hooks"), PathBuf::from("/usr/share/libalpm/hooks")],
            gpg_dir: PathBuf::from("/etc/pacman.d/gnupg"),
            log_file: Some(PathBuf::from("/var/log/pacman.log")),
            architecture: detect_host_arch(),
            check_space: true,
            color: true,
            repositories: Vec::new(),
        }
    }
}

impl PacmanConfig {
    pub fn load_default(cli_config: Option<&str>, cli_root: Option<&str>, cli_dbpath: Option<&str>, no_cachy: bool) -> Result<Self> {
        let conf_path = cli_config.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/etc/pacman.conf"));
        let mut config = if conf_path.exists() {
            Self::parse_file(&conf_path)?
        } else {
            Self::default()
        };

        // Apply CLI overrides if specified
        if let Some(r) = cli_root {
            let root_buf = PathBuf::from(r);
            // If db_path was default, adjust relative to new root
            if config.db_path == PathBuf::from("/var/lib/pacman") {
                config.db_path = root_buf.join("var/lib/pacman");
            }
            config.root_dir = root_buf;
        }

        if let Some(db) = cli_dbpath {
            config.db_path = PathBuf::from(db);
        }

        // Ensure standard upstream Arch repositories are present
        config.ensure_upstream_repositories();

        // Inject or filter CachyOS repositories based on --no-cachy flag
        if no_cachy {
            config.repositories.retain(|r| !r.name.starts_with("cachyos"));
        } else {
            config.inject_cachy_repositories();
        }

        Ok(config)
    }

    pub fn inject_cachy_repositories(&mut self) {
        let has_cachy = self.repositories.iter().any(|r| r.name.starts_with("cachyos"));
        if has_cachy {
            return;
        }

        let cpu_tier = detect_cpu_tier();
        let arch = &self.architecture;

        let mut injected = Vec::new();

        match cpu_tier {
            CpuTier::V4 => {
                injected.push(Repository {
                    name: "cachyos-v4".to_string(),
                    servers: vec![
                        "https://mirror.cachyos.org/repo/x86_64_v4/cachyos-v4".to_string(),
                        format!("https://geo.mirror.pkgbuild.com/cachyos-v4/cachyos-v4/os/{}", arch),
                    ],
                    sig_level: None,
                });
                injected.push(Repository {
                    name: "cachyos-extra-v4".to_string(),
                    servers: vec![
                        "https://mirror.cachyos.org/repo/x86_64_v4/cachyos-extra-v4".to_string(),
                        format!("https://geo.mirror.pkgbuild.com/cachyos-v4/cachyos-extra-v4/os/{}", arch),
                    ],
                    sig_level: None,
                });
                injected.push(Repository {
                    name: "cachyos-core-v4".to_string(),
                    servers: vec![
                        "https://mirror.cachyos.org/repo/x86_64_v4/cachyos-core-v4".to_string(),
                        format!("https://geo.mirror.pkgbuild.com/cachyos-v4/cachyos-core-v4/os/{}", arch),
                    ],
                    sig_level: None,
                });
            }
            CpuTier::V3 => {
                injected.push(Repository {
                    name: "cachyos-v3".to_string(),
                    servers: vec![
                        "https://mirror.cachyos.org/repo/x86_64_v3/cachyos-v3".to_string(),
                        format!("https://geo.mirror.pkgbuild.com/cachyos-v3/cachyos-v3/os/{}", arch),
                    ],
                    sig_level: None,
                });
                injected.push(Repository {
                    name: "cachyos-extra-v3".to_string(),
                    servers: vec![
                        "https://mirror.cachyos.org/repo/x86_64_v3/cachyos-extra-v3".to_string(),
                        format!("https://geo.mirror.pkgbuild.com/cachyos-v3/cachyos-extra-v3/os/{}", arch),
                    ],
                    sig_level: None,
                });
                injected.push(Repository {
                    name: "cachyos-core-v3".to_string(),
                    servers: vec![
                        "https://mirror.cachyos.org/repo/x86_64_v3/cachyos-core-v3".to_string(),
                        format!("https://geo.mirror.pkgbuild.com/cachyos-v3/cachyos-core-v3/os/{}", arch),
                    ],
                    sig_level: None,
                });
            }
            CpuTier::Generic => {}
        }

        injected.push(Repository {
            name: "cachyos".to_string(),
            servers: vec![
                format!("https://mirror.cachyos.org/repo/{}/cachyos", arch),
                format!("https://geo.mirror.pkgbuild.com/cachyos/cachyos/os/{}", arch),
            ],
            sig_level: None,
        });

        // Insert at the beginning so CachyOS optimized packages take precedence
        for (i, repo) in injected.into_iter().enumerate() {
            self.repositories.insert(i, repo);
        }
    }

    pub fn ensure_upstream_repositories(&mut self) {
        let arch = &self.architecture;
        let default_arch_repos = [
            ("core", "https://geo.mirror.pkgbuild.com/$repo/os/$arch"),
            ("extra", "https://geo.mirror.pkgbuild.com/$repo/os/$arch"),
            ("multilib", "https://geo.mirror.pkgbuild.com/$repo/os/$arch"),
        ];

        for (name, server_template) in default_arch_repos {
            if !self.repositories.iter().any(|r| r.name == name) {
                let expanded = expand_variables(server_template, name, arch);
                self.repositories.push(Repository {
                    name: name.to_string(),
                    servers: vec![expanded],
                    sig_level: None,
                });
            }
        }
    }

    pub fn parse_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read pacman config at {:?}", path))?;

        let mut config = Self::default();
        let mut current_section: Option<String> = None;
        let mut current_repo: Option<Repository> = None;

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Section Header: [section_name]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let section_name = trimmed[1..trimmed.len() - 1].trim().to_string();

                // Save previous repo if any
                if let Some(repo) = current_repo.take() {
                    if !repo.servers.is_empty() {
                        config.repositories.push(repo);
                    }
                }

                if section_name.eq_ignore_ascii_case("options") {
                    current_section = Some("options".to_string());
                } else {
                    current_section = Some(section_name.clone());
                    current_repo = Some(Repository {
                        name: section_name,
                        servers: Vec::new(),
                        sig_level: None,
                    });
                }
                continue;
            }

            // Key-Value parsing
            let mut parts = trimmed.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let val = parts.next().map(|v| v.trim()).unwrap_or("");

            match current_section.as_deref() {
                Some("options") => match key.to_lowercase().as_str() {
                    "rootdir" => {
                        if !val.is_empty() {
                            config.root_dir = PathBuf::from(val);
                        }
                    }
                    "dbpath" => {
                        if !val.is_empty() {
                            config.db_path = PathBuf::from(val);
                        }
                    }
                    "cachedir" => {
                        if !val.is_empty() {
                            config.cache_dirs = val.split_whitespace().map(PathBuf::from).collect();
                        }
                    }
                    "architecture" => {
                        if !val.is_empty() && val != "auto" {
                            config.architecture = val.to_string();
                        }
                    }
                    "checkspace" => {
                        config.check_space = true;
                    }
                    "color" => {
                        config.color = true;
                    }
                    "include" => {
                        if !val.is_empty() {
                            // Parse included option fragments
                            let inc_path = Path::new(val);
                            if inc_path.exists() {
                                if let Ok(inc_conf) = Self::parse_file(inc_path) {
                                    for repo in inc_conf.repositories {
                                        config.repositories.push(repo);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                },
                Some(_repo_name) => {
                    if let Some(ref mut repo) = current_repo {
                        match key.to_lowercase().as_str() {
                            "server" => {
                                if !val.is_empty() {
                                    let expanded = expand_variables(val, &repo.name, &config.architecture);
                                    repo.servers.push(expanded);
                                }
                            }
                            "siglevel" => {
                                if !val.is_empty() {
                                    repo.sig_level = Some(val.to_string());
                                }
                            }
                            "include" => {
                                if !val.is_empty() {
                                    let inc_path = Path::new(val);
                                    if inc_path.exists() {
                                        let servers = parse_mirrorlist(inc_path, &repo.name, &config.architecture);
                                        repo.servers.extend(servers);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                None => {}
            }
        }

        // Save last repository
        if let Some(repo) = current_repo.take() {
            if !repo.servers.is_empty() {
                config.repositories.push(repo);
            }
        }

        Ok(config)
    }
}

fn expand_variables(url: &str, repo: &str, arch: &str) -> String {
    url.replace("$repo", repo).replace("$arch", arch)
}

fn parse_mirrorlist(path: &Path, repo: &str, arch: &str) -> Vec<String> {
    let mut servers = Vec::new();
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if let Some(val) = trimmed.strip_prefix("Server = ") {
                let expanded = expand_variables(val.trim(), repo, arch);
                servers.push(expanded);
            } else if let Some(val) = trimmed.strip_prefix("Server=") {
                let expanded = expand_variables(val.trim(), repo, arch);
                servers.push(expanded);
            }
        }
    }
    servers
}

fn detect_host_arch() -> String {
    #[cfg(target_arch = "x86_64")]
    return "x86_64".to_string();

    #[cfg(target_arch = "aarch64")]
    return "aarch64".to_string();

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return "x86_64".to_string();
}
