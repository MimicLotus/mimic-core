pub mod search;
pub mod sync;
pub mod trans;

use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use colored::*;

use crate::config::PacmanConfig;

pub struct AlpmEngine {
    pub handle: alpm::Alpm,
    pub config: PacmanConfig,
    pub is_sandbox: bool,
}

impl AlpmEngine {
    pub fn new(config: PacmanConfig) -> Result<Self> {
        let is_sandbox = config.root_dir != Path::new("/");

        // Ensure directories exist (crucial for sandbox roots)
        if !config.root_dir.exists() {
            fs::create_dir_all(&config.root_dir)
                .with_context(|| format!("Failed to create root directory at {:?}", config.root_dir))?;
        }

        let local_db_dir = config.db_path.join("local");
        if !local_db_dir.exists() {
            fs::create_dir_all(&local_db_dir)
                .with_context(|| format!("Failed to create local db directory at {:?}", local_db_dir))?;
        }

        let sync_db_dir = config.db_path.join("sync");
        if !sync_db_dir.exists() {
            fs::create_dir_all(&sync_db_dir)
                .with_context(|| format!("Failed to create sync db directory at {:?}", sync_db_dir))?;
        }

        for cache_dir in &config.cache_dirs {
            if !cache_dir.exists() {
                let _ = fs::create_dir_all(cache_dir);
            }
        }

        let root_str = config.root_dir.to_str().unwrap_or("/");
        let dbpath_str = config.db_path.to_str().unwrap_or("/var/lib/pacman");

        let handle = alpm::Alpm::new(root_str, dbpath_str)
            .with_context(|| format!("Failed to initialize ALPM handle (root: {}, dbpath: {})", root_str, dbpath_str))?;

        let mut engine = Self {
            handle,
            config,
            is_sandbox,
        };

        // Register repositories from configuration
        engine.register_configured_repositories()?;

        Ok(engine)
    }

    fn register_configured_repositories(&mut self) -> Result<()> {
        for repo in &self.config.repositories {
            // Register sync database
            let siglevel = alpm::SigLevel::USE_DEFAULT;
            match self.handle.register_syncdb_mut(repo.name.as_str(), siglevel) {
                Ok(db) => {
                    for server in &repo.servers {
                        let _ = db.add_server(server.as_str());
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{} Warning: Failed to register repository '{}': {}",
                        "⚠️".yellow(),
                        repo.name.bold(),
                        e
                    );
                }
            }
        }
        Ok(())
    }
}
