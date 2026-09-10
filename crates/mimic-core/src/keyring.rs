use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use anyhow::{Context, Result};
use colored::*;

use crate::config::PacmanConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyValidity {
    Ultimate,
    Full,
    Marginal,
    Expired,
    Revoked,
    Disabled,
    Invalid,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct GpgKey {
    pub key_id: String,
    pub fingerprint: String,
    pub key_type: String,
    pub key_length: u32,
    pub validity: KeyValidity,
    pub created: Option<String>,
    pub expires: Option<String>,
    pub uids: Vec<String>,
    pub is_master: bool,
}

pub struct KeyringManager {
    pub gpg_dir: PathBuf,
    pub keyring_import_dir: PathBuf,
    pub keyserver: Option<String>,
}

impl KeyringManager {
    pub fn new(config: &PacmanConfig) -> Self {
        let gpg_dir = config.gpg_dir.clone();
        let keyring_import_dir = config.root_dir.join("usr/share/pacman/keyrings");
        Self {
            gpg_dir,
            keyring_import_dir,
            keyserver: Some("hkps://keyserver.ubuntu.com".to_string()),
        }
    }

    #[allow(dead_code)]
    pub fn with_dirs(gpg_dir: &Path, keyring_import_dir: &Path) -> Self {
        Self {
            gpg_dir: gpg_dir.to_path_buf(),
            keyring_import_dir: keyring_import_dir.to_path_buf(),
            keyserver: Some("hkps://keyserver.ubuntu.com".to_string()),
        }
    }

    fn gpg_cmd(&self) -> Result<Command> {
        let gpg_bin = which::which("gpg")
            .or_else(|_| which::which("gpg2"))
            .context("GnuPG binary ('gpg') was not found on the system. Please ensure gnupg is installed.")?;
        let mut cmd = Command::new(gpg_bin);
        cmd.arg("--homedir").arg(&self.gpg_dir);
        cmd.arg("--no-permission-warning");
        Ok(cmd)
    }

    fn ensure_writable(&self) -> Result<()> {
        if self.gpg_dir.exists() {
            let test_file = self.gpg_dir.join(".mimic_write_test");
            match fs::OpenOptions::new().write(true).create(true).open(&test_file) {
                Ok(_) => {
                    let _ = fs::remove_file(test_file);
                    Ok(())
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    anyhow::bail!(
                        "Insufficient permissions to modify the GPG keyring at '{}'.\n\
                         Please run with elevated privileges (e.g. 'sudo mimic key ...').",
                        self.gpg_dir.display()
                    );
                }
                Err(e) => Err(e).with_context(|| format!("Failed to access GPG directory at {:?}", self.gpg_dir)),
            }
        } else if let Some(parent) = self.gpg_dir.parent() {
            if parent.exists() {
                let test_file = parent.join(".mimic_write_test");
                match fs::OpenOptions::new().write(true).create(true).open(&test_file) {
                    Ok(_) => {
                        let _ = fs::remove_file(test_file);
                        Ok(())
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        anyhow::bail!(
                            "Insufficient permissions to create GPG keyring at '{}'.\n\
                             Please run with elevated privileges (e.g. 'sudo mimic key ...').",
                            self.gpg_dir.display()
                        );
                    }
                    Err(e) => Err(e).with_context(|| format!("Failed to access parent directory at {:?}", parent)),
                }
            } else {
                Ok(())
            }
        } else {
            Ok(())
        }
    }

    /// 🔑 Initialize the system GPG trust database
    pub fn init(&self) -> Result<()> {
        println!(
            "{} Initializing pacman GPG trust database at '{}'...",
            "::".cyan().bold(),
            self.gpg_dir.display().to_string().bold()
        );

        self.ensure_writable()?;

        // 1. Create keyring directory
        if !self.gpg_dir.exists() {
            fs::create_dir_all(&self.gpg_dir)
                .with_context(|| format!("Failed to create GPG directory at {:?}", self.gpg_dir))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.gpg_dir, fs::Permissions::from_mode(0o755));
            }
        }

        // 2. Touch pubring and secring files if not existing
        let pubring = self.gpg_dir.join("pubring.gpg");
        if !pubring.exists() {
            fs::File::create(&pubring)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&pubring, fs::Permissions::from_mode(0o644));
            }
        }

        let secring = self.gpg_dir.join("secring.gpg");
        if !secring.exists() {
            fs::File::create(&secring)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&secring, fs::Permissions::from_mode(0o600));
            }
        }

        // 3. Configure gpg.conf
        let gpg_conf = self.gpg_dir.join("gpg.conf");
        let mut conf_content = if gpg_conf.exists() {
            fs::read_to_string(&gpg_conf).unwrap_or_default()
        } else {
            String::new()
        };

        let required_opts = [
            "no-greeting",
            "no-permission-warning",
            "keyserver-options timeout=10",
            "keyserver-options import-clean",
            "keyserver-options no-self-sigs-only",
        ];

        let mut conf_updated = false;
        for opt in &required_opts {
            if !conf_content.lines().any(|line| line.trim() == *opt) {
                conf_content.push_str(opt);
                conf_content.push('\n');
                conf_updated = true;
            }
        }

        if conf_updated || !gpg_conf.exists() {
            fs::write(&gpg_conf, &conf_content)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&gpg_conf, fs::Permissions::from_mode(0o644));
            }
        }

        // 4. Configure gpg-agent.conf
        let agent_conf = self.gpg_dir.join("gpg-agent.conf");
        let mut agent_content = if agent_conf.exists() {
            fs::read_to_string(&agent_conf).unwrap_or_default()
        } else {
            String::new()
        };

        if !agent_content.lines().any(|line| line.trim() == "disable-scdaemon") {
            agent_content.push_str("disable-scdaemon\n");
            fs::write(&agent_conf, &agent_content)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&agent_conf, fs::Permissions::from_mode(0o644));
            }
        }

        // 5. Initialize or update trustdb
        let mut check_db = self.gpg_cmd()?;
        check_db.args(["--batch", "--check-trustdb"]);
        let _ = check_db.status();

        // 6. Check if a secret master key exists
        let has_secret_key = self.has_secret_key()?;
        if !has_secret_key {
            println!("  • Generating pacman keyring master key (RSA 4096)...");
            self.generate_master_key()?;
            println!("  {} Master signing key generated.", "✔".green().bold());
        } else {
            println!("  {} Pacman master signing key is present.", "✔".green().bold());
        }

        println!("\n{} Pacman GPG trust database initialized successfully.\n", "✔".green().bold());
        Ok(())
    }

    fn has_secret_key(&self) -> Result<bool> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["-K", "--with-colons"]);
        let output = cmd.output()?;
        let count = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.starts_with("sec:"))
            .count();
        Ok(count > 0)
    }

    fn generate_master_key(&self) -> Result<()> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--batch", "--gen-key"]);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn gpg --gen-key")?;
        if let Some(mut stdin) = child.stdin.take() {
            let script = "Key-Type: RSA\n\
                          Key-Length: 4096\n\
                          Key-Usage: sign\n\
                          Name-Real: Pacman Keyring Master Key\n\
                          Name-Email: pacman@localhost\n\
                          Expire-Date: 0\n\
                          %no-protection\n\
                          %commit\n";
            let _ = stdin.write_all(script.as_bytes());
        }

        let status = child.wait().context("Failed to wait on gpg keygen")?;
        if !status.success() {
            anyhow::bail!("Failed to generate master signing key (exit: {})", status);
        }

        let mut update_db = self.gpg_cmd()?;
        update_db.args(["--batch", "--check-trustdb"]);
        let _ = update_db.status();

        Ok(())
    }

    /// 📥 Populate trusted keys from /usr/share/pacman/keyrings/
    pub fn populate(&self, names: &[String]) -> Result<()> {
        println!(
            "{} Populating trusted pacman keyrings from '{}'...",
            "::".cyan().bold(),
            self.keyring_import_dir.display().to_string().bold()
        );

        self.ensure_writable()?;

        if !self.keyring_import_dir.exists() {
            anyhow::bail!(
                "Keyring directory '{}' does not exist. Ensure archlinux-keyring is installed.",
                self.keyring_import_dir.display()
            );
        }

        // Determine keyrings to populate
        let keyring_names: Vec<String> = if names.is_empty() {
            let mut found = Vec::new();
            if let Ok(entries) = fs::read_dir(&self.keyring_import_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.ends_with(".gpg") {
                                found.push(name.trim_end_matches(".gpg").to_string());
                            }
                        }
                    }
                }
            }
            found.sort();
            if found.is_empty() {
                anyhow::bail!("No .gpg keyring files found in '{}'", self.keyring_import_dir.display());
            }
            found
        } else {
            for name in names {
                let gpg_file = self.keyring_import_dir.join(format!("{}.gpg", name));
                if !gpg_file.exists() {
                    anyhow::bail!("Keyring file '{}' does not exist.", gpg_file.display());
                }
            }
            names.to_vec()
        };

        let master_key_fpr = self.get_master_key_fingerprint().ok();

        for keyring in &keyring_names {
            println!("  • {} Keyring: {}", "🔑".yellow(), keyring.bold().cyan());

            // 1. Import .gpg public keys
            let gpg_file = self.keyring_import_dir.join(format!("{}.gpg", keyring));
            let mut import_cmd = self.gpg_cmd()?;
            import_cmd.args(["--quiet", "--batch", "--import", gpg_file.to_str().unwrap()]);
            let status = import_cmd.status().with_context(|| format!("Failed to import keyring {}", gpg_file.display()))?;
            if !status.success() {
                eprintln!("    {} Warning: gpg import returned exit code {}", "⚠️".yellow(), status);
            }

            // 2. Locally sign trusted keys
            let trusted_file = self.keyring_import_dir.join(format!("{}-trusted", keyring));
            if trusted_file.exists() {
                let trusted_content = fs::read_to_string(&trusted_file).unwrap_or_default();
                let mut signed_count = 0;

                for line in trusted_content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    let fprint = trimmed.split(':').next().unwrap_or("").trim();
                    if fprint.is_empty() {
                        continue;
                    }

                    if !self.key_is_lsigned(fprint, master_key_fpr.as_deref())? {
                        if self.lsign_key(fprint)? {
                            signed_count += 1;
                        }
                    }
                }

                if signed_count > 0 {
                    println!("    ✔ Locally signed {} trusted key(s).", signed_count);
                }

                // Import ownertrust values
                let mut trust_cmd = self.gpg_cmd()?;
                trust_cmd.args(["--batch", "--import-ownertrust", trusted_file.to_str().unwrap()]);
                let _ = trust_cmd.status();
            }

            // 3. Disable revoked keys
            let revoked_file = self.keyring_import_dir.join(format!("{}-revoked", keyring));
            if revoked_file.exists() {
                let revoked_content = fs::read_to_string(&revoked_file).unwrap_or_default();
                let mut revoked_count = 0;

                for line in revoked_content.lines() {
                    let fprint = line.trim();
                    if fprint.is_empty() || fprint.starts_with('#') {
                        continue;
                    }

                    if !self.key_is_disabled(fprint)? {
                        if self.disable_key(fprint)? {
                            revoked_count += 1;
                        }
                    }
                }

                if revoked_count > 0 {
                    println!("    ✔ Disabled {} revoked key(s).", revoked_count);
                }
            }
        }

        // Update trustdb
        let mut update_db = self.gpg_cmd()?;
        update_db.args(["--batch", "--check-trustdb"]);
        let _ = update_db.status();

        println!("\n{} Successfully populated trusted pacman keyrings.\n", "✔".green().bold());
        Ok(())
    }

    fn get_master_key_fingerprint(&self) -> Result<String> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--list-secret-keys", "--with-colons"]);
        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() > 9 && parts[0] == "fpr" && !parts[9].is_empty() {
                return Ok(parts[9].to_string());
            }
        }

        anyhow::bail!("Pacman master signing key fingerprint not found")
    }

    fn key_is_lsigned(&self, fprint: &str, master_key_fpr: Option<&str>) -> Result<bool> {
        let master_fpr = match master_key_fpr {
            Some(fpr) => fpr.to_string(),
            None => match self.get_master_key_fingerprint() {
                Ok(fpr) => fpr,
                Err(_) => return Ok(false),
            },
        };

        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--with-colons", "--check-signatures", "--quiet", fprint]);
        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() > 4 && parts[0] == "sig" && parts[1] == "!" {
                let signing_key = parts[4];
                if master_fpr.ends_with(signing_key) || signing_key == master_fpr {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn lsign_key(&self, fprint: &str) -> Result<bool> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--command-fd", "0", "--quiet", "--batch", "--lsign-key", fprint]);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"y\ny\n");
        }

        let status = child.wait()?;
        Ok(status.success())
    }

    fn key_is_disabled(&self, fprint: &str) -> Result<bool> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--with-colons", "--list-key", "--quiet", fprint]);
        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() > 11 && parts[0] == "pub" {
                if parts[11].contains('D') {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn disable_key(&self, fprint: &str) -> Result<bool> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--command-fd", "0", "--no-auto-check-trustdb", "--quiet", "--batch", "--edit-key", fprint]);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"disable\nquit\n");
        }

        let status = child.wait()?;
        Ok(status.success())
    }

    /// 🔄 Synchronize keyring packages, repopulate, and refresh from keyserver
    pub fn sync(&self) -> Result<()> {
        println!("{} Synchronizing GPG keyrings and trust databases...", "::".cyan().bold());

        self.ensure_writable()?;

        // 1. Repopulate local keyrings from /usr/share/pacman/keyrings/
        self.populate(&[])?;

        // 2. Refresh trusted keys from keyserver
        println!("{} Refreshing signing keys from keyserver...", "::".cyan().bold());
        let master_fpr = self.get_master_key_fingerprint().ok();

        let keys = self.get_all_keys()?;
        let refresh_keys: Vec<String> = keys
            .iter()
            .filter(|k| {
                if let Some(ref m_fpr) = master_fpr {
                    &k.fingerprint != m_fpr
                } else {
                    !k.is_master
                }
            })
            .map(|k| k.fingerprint.clone())
            .collect();

        if !refresh_keys.is_empty() {
            println!("  • Refreshing {} public signing key(s)...", refresh_keys.len());

            let mut refresh_cmd = self.gpg_cmd()?;
            if let Some(ref ks) = self.keyserver {
                refresh_cmd.args(["--keyserver", ks]);
            }
            refresh_cmd.args(["--batch", "--refresh-keys"]);
            for k in refresh_keys.iter().take(50) {
                refresh_cmd.arg(k);
            }

            match refresh_cmd.status() {
                Ok(status) if status.success() => {
                    println!("  {} Keys refreshed successfully from keyserver.", "✔".green().bold());
                }
                Ok(status) => {
                    println!("  {} Note: Keyserver refresh exited with code {} (keyservers may be unreachable).", "⚠️".yellow(), status);
                }
                Err(e) => {
                    println!("  {} Note: Failed to contact keyserver: {}", "⚠️".yellow(), e);
                }
            }
        }

        // 3. Final trustdb verification
        let mut update_db = self.gpg_cmd()?;
        update_db.args(["--batch", "--check-trustdb"]);
        let _ = update_db.status();

        println!("\n{} Keyring synchronization complete.\n", "✔".green().bold());
        Ok(())
    }

    /// 🔍 List trusted signing keys
    pub fn list(&self) -> Result<()> {
        macro_rules! wprint {
            ($($arg:tt)*) => {
                if let Err(e) = write!(std::io::stdout(), $($arg)*) {
                    if e.kind() == std::io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                }
            };
        }
        macro_rules! wprintln {
            () => {
                wprint!("\n");
            };
            ($($arg:tt)*) => {
                if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
                    if e.kind() == std::io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                }
            };
        }

        wprintln!("{} Reading pacman trusted GPG key database from '{}'...\n", "::".cyan().bold(), self.gpg_dir.display().to_string().cyan());

        let keys = self.get_all_keys()?;
        if keys.is_empty() {
            wprintln!("{} No keys found in keyring at '{}'.", "⚠️".yellow(), self.gpg_dir.display());
            wprintln!("  Run 'mimic key init' and 'mimic key populate' to initialize trusted keys.\n");
            return Ok(());
        }

        wprintln!("{}\n", "👑 PACMAN TRUSTED GPG KEYRING".magenta().bold());

        let total = keys.len();
        let trusted_count = keys.iter().filter(|k| matches!(k.validity, KeyValidity::Ultimate | KeyValidity::Full)).count();
        let revoked_count = keys.iter().filter(|k| matches!(k.validity, KeyValidity::Revoked | KeyValidity::Disabled)).count();

        wprintln!(
            "  Directory : {}\n  Total Keys: {} (Trusted: {}, Revoked/Disabled: {})\n",
            self.gpg_dir.display().to_string().cyan(),
            total.to_string().bold().green(),
            trusted_count.to_string().bold().green(),
            revoked_count.to_string().bold().red()
        );

        for (i, key) in keys.iter().enumerate() {
            let validity_badge = match key.validity {
                KeyValidity::Ultimate => "[ Ultimate ]".bold().magenta(),
                KeyValidity::Full => "[ Trusted  ]".bold().green(),
                KeyValidity::Marginal => "[ Marginal ]".bold().yellow(),
                KeyValidity::Expired => "[ Expired  ]".bold().red(),
                KeyValidity::Revoked => "[ Revoked  ]".bold().red(),
                KeyValidity::Disabled => "[ Disabled ]".dimmed(),
                KeyValidity::Invalid => "[ Invalid  ]".bold().red(),
                KeyValidity::Unknown => "[ Unknown  ]".dimmed(),
            };

            let primary_uid = key.uids.first().map(|s| s.as_str()).unwrap_or("Unknown");
            let type_str = format!("{}/{}", key.key_type, key.key_length);

            wprintln!(
                " {:>3}. {} {:<14} {}",
                (i + 1).to_string().dimmed(),
                validity_badge,
                type_str.cyan(),
                primary_uid.bold()
            );
            wprintln!("      Key ID:      {}", key.key_id.bold().yellow());
            wprintln!("      Fingerprint: {}", format_fingerprint(&key.fingerprint).dimmed());

            if let Some(ref created) = key.created {
                wprint!("      Created:     {}", created);
                if let Some(ref expires) = key.expires {
                    wprint!(" | Expires: {}", expires.yellow());
                }
                wprintln!();
            }

            if key.uids.len() > 1 {
                for alt in &key.uids[1..] {
                    wprintln!("      uid:         {}", alt.dimmed());
                }
            }
            wprintln!();
        }

        Ok(())
    }

    pub fn get_all_keys(&self) -> Result<Vec<GpgKey>> {
        let mut cmd = self.gpg_cmd()?;
        cmd.args(["--list-keys", "--with-colons"]);
        let output = cmd.output().context("Failed to execute gpg --list-keys")?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut keys: Vec<GpgKey> = Vec::new();
        let mut current: Option<GpgKey> = None;

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "pub" => {
                    if let Some(k) = current.take() {
                        keys.push(k);
                    }

                    let val_code = parts.get(1).unwrap_or(&"");
                    let flags = parts.get(11).unwrap_or(&"");
                    let mut validity = match *val_code {
                        "u" => KeyValidity::Ultimate,
                        "f" => KeyValidity::Full,
                        "m" => KeyValidity::Marginal,
                        "e" => KeyValidity::Expired,
                        "r" => KeyValidity::Revoked,
                        "d" => KeyValidity::Disabled,
                        "i" => KeyValidity::Invalid,
                        _ => KeyValidity::Unknown,
                    };
                    if flags.contains('D') {
                        validity = KeyValidity::Disabled;
                    }

                    let key_length = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                    let key_algo = parts.get(3).unwrap_or(&"");
                    let key_type = match *key_algo {
                        "1" => "rsa",
                        "17" => "dsa",
                        "18" => "ecdh",
                        "19" => "ecdsa",
                        "22" => "ed25519",
                        _ => "pub",
                    }.to_string();

                    let key_id = parts.get(4).unwrap_or(&"").to_string();
                    let created = parts.get(5).and_then(|s| format_unix_timestamp(s));
                    let expires = parts.get(6).and_then(|s| format_unix_timestamp(s));

                    current = Some(GpgKey {
                        key_id,
                        fingerprint: String::new(),
                        key_type,
                        key_length,
                        validity,
                        created,
                        expires,
                        uids: Vec::new(),
                        is_master: false,
                    });
                }
                "fpr" => {
                    if let Some(ref mut k) = current {
                        if k.fingerprint.is_empty() {
                            if let Some(fpr) = parts.get(9) {
                                k.fingerprint = fpr.to_string();
                            }
                        }
                    }
                }
                "uid" => {
                    if let Some(ref mut k) = current {
                        if let Some(uid) = parts.get(9) {
                            if !uid.is_empty() {
                                k.uids.push(uid.to_string());
                                if uid.contains("pacman@localhost") || uid.contains("Pacman Keyring Master Key") {
                                    k.is_master = true;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(k) = current {
            keys.push(k);
        }

        Ok(keys)
    }
}

pub fn format_fingerprint(fpr: &str) -> String {
    let clean: String = fpr.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if clean.len() == 40 {
        let chunks: Vec<&str> = (0..10)
            .map(|i| &clean[i * 4..(i + 1) * 4])
            .collect();
        format!(
            "{} {} {} {} {}  {} {} {} {} {}",
            chunks[0], chunks[1], chunks[2], chunks[3], chunks[4],
            chunks[5], chunks[6], chunks[7], chunks[8], chunks[9]
        )
    } else {
        fpr.to_string()
    }
}

pub fn format_unix_timestamp(ts: &str) -> Option<String> {
    let secs = ts.parse::<i64>().ok()?;
    if secs <= 0 {
        return None;
    }
    let mut days = secs / 86400;
    let mut year = 1970;
    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let diy = if is_leap { 366 } else { 365 };
        if days >= diy {
            days -= diy;
            year += 1;
        } else {
            break;
        }
    }
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = [
        31,
        if is_leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 1;
    for &mdays in &month_days {
        if days >= mdays {
            days -= mdays;
            month += 1;
        } else {
            break;
        }
    }
    let day = days + 1;
    Some(format!("{:04}-{:02}-{:02}", year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_unix_timestamp() {
        assert_eq!(format_unix_timestamp("1669893366"), Some("2022-12-01".to_string()));
        assert_eq!(format_unix_timestamp("0"), None);
        assert_eq!(format_unix_timestamp("-100"), None);
        assert_eq!(format_unix_timestamp("invalid"), None);
    }

    #[test]
    fn test_format_fingerprint() {
        let fpr = "6EFFE9382C1406BB8C71591FF7BFE93A8C440483";
        let formatted = format_fingerprint(fpr);
        assert_eq!(formatted, "6EFF E938 2C14 06BB 8C71  591F F7BF E93A 8C44 0483");
    }

    #[test]
    fn test_keyring_init_and_list_tempdir() {
        let temp_dir = tempfile::tempdir().expect("Failed to create tempdir");
        let gpg_dir = temp_dir.path().join("gnupg");
        let keyring_dir = temp_dir.path().join("keyrings");
        let _ = fs::create_dir_all(&keyring_dir);

        let mgr = KeyringManager::with_dirs(&gpg_dir, &keyring_dir);
        let init_res = mgr.init();
        assert!(init_res.is_ok(), "Keyring init failed: {:?}", init_res.err());

        assert!(gpg_dir.join("pubring.gpg").exists());
        assert!(gpg_dir.join("secring.gpg").exists());
        assert!(gpg_dir.join("gpg.conf").exists());
        assert!(gpg_dir.join("gpg-agent.conf").exists());

        // Check listing returns the master key
        let keys = mgr.get_all_keys().expect("Failed to list keys");
        assert!(!keys.is_empty(), "Expected at least 1 key (master key)");
        assert!(keys.iter().any(|k| k.is_master));
    }
}
