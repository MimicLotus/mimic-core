use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbilityRecord {
    pub id: String,
    pub name: String,
    pub source_type: String, // "deb", "rpm", "git", "aur", "apk"
    pub upstream_url: String,
    pub version: String,
    pub consumed_at: String,
    pub binary_paths: Vec<String>,
    pub companion_libs: Vec<String>,
    pub desktop_files: Vec<String>,
}

pub struct StateLedger {
    conn: Connection,
    pub db_path: PathBuf,
}

impl StateLedger {
    pub fn open() -> Result<Self> {
        let db_path = Self::get_default_db_path();
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open state ledger at {:?}", db_path))?;

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS abilities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_type TEXT NOT NULL,
                upstream_url TEXT NOT NULL,
                version TEXT NOT NULL,
                consumed_at TEXT NOT NULL,
                binary_paths TEXT NOT NULL,
                companion_libs TEXT NOT NULL,
                desktop_files TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_registry (
                path TEXT PRIMARY KEY,
                owner_ability_id TEXT NOT NULL,
                ref_count INTEGER NOT NULL DEFAULT 1
            );
            "#,
        )?;

        Ok(Self { conn, db_path })
    }

    pub fn get_default_db_path() -> PathBuf {
        let global_mimic = Path::new("/mimic/state.db");
        if global_mimic.exists() || is_root_or_writable(Path::new("/mimic")) {
            return global_mimic.to_path_buf();
        }

        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local/share/mimic/state.db")
        } else {
            PathBuf::from("/tmp/mimic/state.db")
        }
    }

    pub fn record_ability(&mut self, record: &AbilityRecord) -> Result<()> {
        let binaries_json = serde_json::to_string(&record.binary_paths)?;
        let libs_json = serde_json::to_string(&record.companion_libs)?;
        let desktop_json = serde_json::to_string(&record.desktop_files)?;

        let tx = self.conn.transaction()?;

        tx.execute(
            r#"
            INSERT OR REPLACE INTO abilities 
            (id, name, source_type, upstream_url, version, consumed_at, binary_paths, companion_libs, desktop_files)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                record.id,
                record.name,
                record.source_type,
                record.upstream_url,
                record.version,
                record.consumed_at,
                binaries_json,
                libs_json,
                desktop_json,
            ],
        )?;

        // Update file registry with ref counting
        let mut all_files = Vec::new();
        all_files.extend(record.binary_paths.iter().cloned());
        all_files.extend(record.companion_libs.iter().cloned());
        all_files.extend(record.desktop_files.iter().cloned());

        for file_path in all_files {
            tx.execute(
                r#"
                INSERT INTO file_registry (path, owner_ability_id, ref_count)
                VALUES (?1, ?2, 1)
                ON CONFLICT(path) DO UPDATE SET ref_count = ref_count + 1
                "#,
                params![file_path, record.id],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn remove_ability(&mut self, id: &str) -> Result<Vec<String>> {
        let tx = self.conn.transaction()?;

        let record_opt: Option<(String, String, String)> = tx
            .query_row(
                "SELECT binary_paths, companion_libs, desktop_files FROM abilities WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let mut files_to_delete = Vec::new();

        if let Some((binaries_json, libs_json, desktop_json)) = record_opt {
            let binaries: Vec<String> = serde_json::from_str(&binaries_json).unwrap_or_default();
            let libs: Vec<String> = serde_json::from_str(&libs_json).unwrap_or_default();
            let desktops: Vec<String> = serde_json::from_str(&desktop_json).unwrap_or_default();

            let mut all_files = Vec::new();
            all_files.extend(binaries);
            all_files.extend(libs);
            all_files.extend(desktops);

            for file_path in all_files {
                let current_ref_count: Option<i64> = tx
                    .query_row(
                        "SELECT ref_count FROM file_registry WHERE path = ?1",
                        params![file_path],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(count) = current_ref_count {
                    if count <= 1 {
                        tx.execute("DELETE FROM file_registry WHERE path = ?1", params![file_path])?;
                        files_to_delete.push(file_path);
                    } else {
                        tx.execute(
                            "UPDATE file_registry SET ref_count = ref_count - 1 WHERE path = ?1",
                            params![file_path],
                        )?;
                    }
                }
            }

            tx.execute("DELETE FROM abilities WHERE id = ?1", params![id])?;
        }

        tx.commit()?;
        Ok(files_to_delete)
    }

    pub fn list_abilities(&self) -> Result<Vec<AbilityRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, source_type, upstream_url, version, consumed_at, binary_paths, companion_libs, desktop_files FROM abilities ORDER BY consumed_at DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            let binaries_json: String = row.get(6)?;
            let libs_json: String = row.get(7)?;
            let desktop_json: String = row.get(8)?;

            Ok(AbilityRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                source_type: row.get(2)?,
                upstream_url: row.get(3)?,
                version: row.get(4)?,
                consumed_at: row.get(5)?,
                binary_paths: serde_json::from_str(&binaries_json).unwrap_or_default(),
                companion_libs: serde_json::from_str(&libs_json).unwrap_or_default(),
                desktop_files: serde_json::from_str(&desktop_json).unwrap_or_default(),
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }
}

fn is_root_or_writable(path: &Path) -> bool {
    if path.exists() {
        if let Ok(metadata) = std::fs::metadata(path) {
            return !metadata.permissions().readonly();
        }
    }
    false
}
