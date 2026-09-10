use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use colored::*;

use super::memory_guard::{MemoryGuard, MemoryProfile};
use super::psi::PsiWatcher;

pub struct SandboxConfig {
    pub build_dir: PathBuf,
    pub pkg_name: String,
    pub use_sccache: bool,
    pub use_mold: bool,
    pub jobs: usize,
    pub memory_profile: MemoryProfile,
}

pub struct BubblewrapSandbox {
    config: SandboxConfig,
}

impl BubblewrapSandbox {
    pub fn new(pkg_name: &str, build_dir: &Path) -> Self {
        let memory_profile = MemoryGuard::assess();
        let jobs = memory_profile.safe_jobs;
        let sccache_exists = which::which("sccache").is_ok() || Path::new("/home/voidlotus/.local/bin/sccache").exists();
        let mold_exists = which::which("mold").map(|p| p.starts_with("/usr")).unwrap_or(false);

        Self {
            config: SandboxConfig {
                build_dir: build_dir.to_path_buf(),
                pkg_name: pkg_name.to_string(),
                use_sccache: sccache_exists,
                use_mold: mold_exists,
                jobs,
                memory_profile,
            },
        }
    }

    #[allow(dead_code)]
    pub fn with_jobs(mut self, jobs: usize) -> Self {
        self.config.jobs = jobs;
        self
    }

    #[allow(dead_code)]
    pub fn jobs(&self) -> usize {
        self.config.jobs
    }

    pub fn execute_build(&self) -> Result<()> {
        self.execute_command(&["makepkg", "-f", "--nodeps"], &[], &[])
    }

    pub fn execute_script(&self, script: &str, extra_binds: &[(&Path, &str)], extra_envs: &[(&str, &str)]) -> Result<()> {
        self.execute_command(&["/bin/sh", "-c", script], extra_binds, extra_envs)
    }

    pub fn execute_command(&self, command_args: &[&str], extra_binds: &[(&Path, &str)], extra_envs: &[(&str, &str)]) -> Result<()> {
        let bwrap_bin = which::which("bwrap")
            .unwrap_or_else(|_| PathBuf::from("/usr/bin/bwrap"));

        if !bwrap_bin.exists() {
            anyhow::bail!("Bubblewrap (bwrap) is not installed on the system.");
        }

        // 1. Setup sccache cache directory on host (NVMe backing)
        let host_cache_dir = dirs_cache_dir().join("sccache");
        let _ = std::fs::create_dir_all(&host_cache_dir);

        // 2. Setup scratch tmp directory on NVMe under build workspace (zero tmpfs RAM consumption)
        let host_scratch_dir = self.config.build_dir.join(".tmp");
        let _ = std::fs::create_dir_all(&host_scratch_dir);

        let host_shims_dir = host_scratch_dir.join("shims");
        let _ = std::fs::create_dir_all(&host_shims_dir);

        // Stage shims for bsdtar and tar to suppress ownership preservation errors (EINVAL) during archive extraction
        let bsdtar_shim = host_shims_dir.join("bsdtar");
        let _ = std::fs::write(&bsdtar_shim, "#!/bin/sh\nexec /usr/bin/bsdtar --no-same-owner \"$@\"\n");
        let tar_shim = host_shims_dir.join("tar");
        let _ = std::fs::write(&tar_shim, "#!/bin/sh\nexec /usr/bin/tar --no-same-owner \"$@\"\n");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&bsdtar_shim, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::set_permissions(&tar_shim, std::fs::Permissions::from_mode(0o755));
        }

        // makepkg shim to bypass EUID == 0 restriction inside fake-root user namespace
        let host_makepkg = Path::new("/usr/bin/makepkg");
        let mut makepkg_shim_path = None;
        if host_makepkg.exists() {
            if let Ok(content) = std::fs::read_to_string(host_makepkg) {
                let patched = content.replace("(( EUID == 0 ))", "(( EUID == 99999 ))");
                let makepkg_shim = host_shims_dir.join("makepkg");
                if std::fs::write(&makepkg_shim, patched).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&makepkg_shim, std::fs::Permissions::from_mode(0o755));
                    }
                    makepkg_shim_path = Some(makepkg_shim);
                }
            }
        }

        let host_local_bin = dirs_local_bin();

        println!(
            "{} Initializing Hermetic Bubblewrap Container for '{}'...",
            "::".cyan().bold(),
            self.config.pkg_name.bold().green()
        );

        if self.config.use_sccache {
            println!("  • {} Shared Compilation Cache enabled: {}", "⚡".yellow(), "sccache".bold().green());
        }
        if self.config.use_mold {
            println!("  • {} High-speed Linker enabled: {}", "⚡".yellow(), "mold (-fuse-ld=mold)".bold().green());
        }
        println!(
            "  • {} CPU Microarchitecture Tuning: {}",
            "⚡".yellow(),
            self.config.memory_profile.tuning_desc.bold().green()
        );
        println!(
            "  • {} MemoryGuard Safe Concurrency: {} jobs (Total RAM: {:.1} GiB)",
            "⚡".yellow(),
            self.config.jobs.to_string().bold().cyan(),
            self.config.memory_profile.total_ram_mib as f64 / 1024.0
        );
        if self.config.memory_profile.is_low_ram {
            println!(
                "  • {} LTO Throttling: {}",
                "⚡".yellow(),
                "Active (LTO stripped to preserve display/system memory headroom)".bold().yellow()
            );
        }

        // Pre-create sccache workspace dir
        let _ = std::fs::create_dir_all(self.config.build_dir.join(".sccache"));

        let mut cmd = Command::new(&bwrap_bin);

        // System read-only root mounts
        cmd.args([
            "--ro-bind", "/usr", "/usr",
            "--symlink", "usr/bin", "/bin",
            "--symlink", "usr/bin", "/sbin",
            "--symlink", "usr/lib", "/lib",
            "--symlink", "usr/lib", "/lib64",
            "--ro-bind", "/etc", "/etc",
            "--ro-bind-try", "/var", "/var",
            "--ro-bind-try", "/sys", "/sys",
            "--proc", "/proc",
            "--dev", "/dev",
            "--tmpfs", "/run",
        ]);

        // Bind patched makepkg shim over /usr/bin/makepkg if available
        if let Some(ref shim) = makepkg_shim_path {
            cmd.args(["--ro-bind", shim.to_str().unwrap(), "/usr/bin/makepkg"]);
        }

        // NVMe-backed scratch directories for /tmp and /var/tmp (zero tmpfs RAM usage)
        cmd.args([
            "--bind", host_scratch_dir.to_str().unwrap(), "/tmp",
            "--bind", host_scratch_dir.to_str().unwrap(), "/var/tmp",
        ]);

        // Stage hermetic container pacman.conf with SigLevel = Never and LocalFileSigLevel = Never
        // to prevent remote sync database signature validation failures during makepkg package assembly
        let host_pacman_conf = Path::new("/etc/pacman.conf");
        let raw_pacman_conf = if host_pacman_conf.exists() {
            std::fs::read_to_string(host_pacman_conf).unwrap_or_default()
        } else {
            String::new()
        };
        let gpg_dir = detect_gpg_dir(&raw_pacman_conf);
        let sandbox_pacman_conf = generate_sandbox_pacman_conf(&raw_pacman_conf);
        let sandbox_pacman_conf_path = host_scratch_dir.join("pacman.conf");
        let _ = std::fs::write(&sandbox_pacman_conf_path, sandbox_pacman_conf);

        // Mount container pacman configuration and keyrings
        cmd.args([
            "--ro-bind", sandbox_pacman_conf_path.to_str().unwrap(), "/etc/pacman.conf",
            "--ro-bind-try", "/etc/pacman.d", "/etc/pacman.d",
            "--ro-bind-try", "/etc/pacman.d/gnupg", "/etc/pacman.d/gnupg",
            "--ro-bind-try", "/usr/share/pacman/keyrings", "/usr/share/pacman/keyrings",
        ]);

        if gpg_dir != Path::new("/etc/pacman.d/gnupg") && gpg_dir.exists() {
            cmd.args([
                "--ro-bind-try", gpg_dir.to_str().unwrap(), gpg_dir.to_str().unwrap(),
            ]);
        }

        // Toolchain mounts
        if host_local_bin.exists() {
            cmd.args([
                "--ro-bind", host_local_bin.to_str().unwrap(), "/usr/local/bin",
                "--ro-bind-try", host_local_bin.to_str().unwrap(), host_local_bin.to_str().unwrap(),
            ]);
        }

        // Workspace and sccache bind mounts
        cmd.args([
            "--bind", self.config.build_dir.to_str().unwrap(), "/build",
            "--bind-try", host_cache_dir.to_str().unwrap(), "/build/.sccache",
        ]);

        // Extra bind mounts (e.g. /staging)
        for (host_p, container_p) in extra_binds {
            let _ = std::fs::create_dir_all(host_p);
            cmd.args(["--bind", host_p.to_str().unwrap(), container_p]);
        }

        // Security & User namespace isolation with fake-root mapping (uid 0 / gid 0)
        cmd.args([
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-uts",
            "--uid", "0",
            "--gid", "0",
            "--chdir", "/build",
            "--clearenv",
        ]);

        // Environment variables
        cmd.args(["--setenv", "PATH", "/tmp/shims:/usr/local/bin:/usr/bin:/bin"]);
        cmd.args(["--setenv", "HOME", "/build"]);
        cmd.args(["--setenv", "USER", "root"]);
        cmd.args(["--setenv", "LOGNAME", "root"]);
        cmd.args(["--setenv", "LANG", "C.UTF-8"]);
        cmd.args(["--setenv", "LC_ALL", "C.UTF-8"]);
        cmd.args(["--setenv", "TMPDIR", "/tmp"]);

        // MemoryGuard adaptive flags and concurrency
        cmd.args(["--setenv", "CFLAGS", &self.config.memory_profile.cflags]);
        cmd.args(["--setenv", "CXXFLAGS", &self.config.memory_profile.cxxflags]);
        cmd.args(["--setenv", "MAKEFLAGS", &format!("-j{}", self.config.jobs)]);
        cmd.args(["--setenv", "NINJAFLAGS", &format!("-j{}", self.config.jobs)]);
        cmd.args(["--setenv", "CARGO_BUILD_JOBS", &format!("{}", self.config.jobs)]);
        cmd.args(["--setenv", "CMAKE_BUILD_PARALLEL_LEVEL", &format!("{}", self.config.jobs)]);

        let mut rustflags = self.config.memory_profile.rustflags.clone();
        let mut ldflags = "-Wl,-O1 -Wl,--sort-common -Wl,--as-needed -Wl,-z,relro -Wl,-z,now".to_string();

        if self.config.use_mold {
            ldflags.push_str(" -fuse-ld=mold");
            rustflags.push_str(" -C link-arg=-fuse-ld=mold");
        }

        cmd.args(["--setenv", "LDFLAGS", &ldflags]);
        cmd.args(["--setenv", "RUSTFLAGS", &rustflags]);

        if self.config.use_sccache {
            cmd.args(["--setenv", "SCCACHE_DIR", "/build/.sccache"]);
            cmd.args(["--setenv", "CC", "sccache gcc"]);
            cmd.args(["--setenv", "CXX", "sccache g++"]);
            cmd.args(["--setenv", "CMAKE_C_COMPILER_LAUNCHER", "sccache"]);
            cmd.args(["--setenv", "CMAKE_CXX_COMPILER_LAUNCHER", "sccache"]);
            cmd.args(["--setenv", "RUSTC_WRAPPER", "sccache"]);
        }

        for (k, v) in extra_envs {
            cmd.args(["--setenv", k, v]);
        }

        // Build command inside bwrap container
        cmd.args(command_args);

        // Put child in its own process group so PSI pressure watcher can signal all workers
        cmd.process_group(0);

        println!("{} Starting unprivileged compilation inside sandbox...\n", "::".green().bold());

        let mut child = cmd.spawn()
            .with_context(|| format!("Failed to execute bwrap process for '{}'", self.config.pkg_name))?;

        let pgid = child.id() as libc::pid_t;
        let watcher = PsiWatcher::start(pgid);

        let status = child.wait()
            .with_context(|| format!("Failed to wait on bwrap process for '{}'", self.config.pkg_name))?;

        watcher.stop();

        if !status.success() {
            anyhow::bail!("Compilation failed with exit code: {}", status);
        }

        println!("\n{} Compilation completed successfully.", "✔".green().bold());
        Ok(())
    }
}

fn dirs_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache/mimic")
    } else {
        PathBuf::from("/var/tmp/mimic-cache")
    }
}

fn dirs_local_bin() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/bin")
    } else {
        PathBuf::from("/usr/local/bin")
    }
}

/// Transforms a pacman.conf by setting SigLevel, LocalFileSigLevel, and RemoteFileSigLevel to Never.
/// This prevents makepkg and pacman -Qi from failing during containerized package assembly
/// when host repository database signatures are out-of-sync or unverified.
pub fn generate_sandbox_pacman_conf(raw_conf: &str) -> String {
    if raw_conf.trim().is_empty() {
        return "[options]\nRootDir = /\nDBPath = /var/lib/pacman/\nCacheDir = /var/cache/pacman/pkg/\nGPGDir = /etc/pacman.d/gnupg/\nArchitecture = auto\nSigLevel = Never\nLocalFileSigLevel = Never\nRemoteFileSigLevel = Never\n".to_string();
    }

    let mut lines = Vec::new();
    let mut has_siglevel = false;
    let mut has_localfile_siglevel = false;

    for line in raw_conf.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();

        if !trimmed.starts_with('#') && lower.starts_with("siglevel") && lower.contains('=') {
            lines.push("SigLevel = Never".to_string());
            has_siglevel = true;
        } else if (lower.starts_with("localfilesiglevel")
            || lower.starts_with("#localfilesiglevel")
            || lower.starts_with("# localfilesiglevel"))
            && lower.contains('=')
        {
            lines.push("LocalFileSigLevel = Never".to_string());
            has_localfile_siglevel = true;
        } else if (lower.starts_with("remotefilesiglevel")
            || lower.starts_with("#remotefilesiglevel")
            || lower.starts_with("# remotefilesiglevel"))
            && lower.contains('=')
        {
            lines.push("RemoteFileSigLevel = Never".to_string());
        } else {
            lines.push(line.to_string());
        }
    }

    if !has_siglevel || !has_localfile_siglevel {
        let mut final_lines = Vec::new();
        let mut inserted = false;
        for line in lines {
            let trimmed = line.trim();
            final_lines.push(line.clone());
            if !inserted && trimmed.eq_ignore_ascii_case("[options]") {
                if !has_siglevel {
                    final_lines.push("SigLevel = Never".to_string());
                }
                if !has_localfile_siglevel {
                    final_lines.push("LocalFileSigLevel = Never".to_string());
                }
                inserted = true;
            }
        }
        if !inserted {
            final_lines.insert(
                0,
                format!(
                    "[options]\n{}{}",
                    if !has_siglevel { "SigLevel = Never\n" } else { "" },
                    if !has_localfile_siglevel { "LocalFileSigLevel = Never\n" } else { "" }
                ),
            );
        }
        final_lines.join("\n")
    } else {
        lines.join("\n")
    }
}

/// Detects the GPG directory configured in pacman.conf, defaulting to `/etc/pacman.d/gnupg`.
pub fn detect_gpg_dir(raw_conf: &str) -> PathBuf {
    for line in raw_conf.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
        if parts.len() == 2 && parts[0].trim().eq_ignore_ascii_case("gpgdir") {
            let val = parts[1].trim();
            if !val.is_empty() {
                return PathBuf::from(val);
            }
        }
    }
    PathBuf::from("/etc/pacman.d/gnupg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_sandbox_pacman_conf_overrides_siglevels() {
        let sample = r#"
[options]
RootDir = /
DBPath = /var/lib/pacman/
SigLevel = Required DatabaseOptional
LocalFileSigLevel = Optional
#RemoteFileSigLevel = Required

[cachyos-extra-v3]
Include = /etc/pacman.d/cachyos-v3-mirrorlist
SigLevel = PackageRequired

[extra]
Include = /etc/pacman.d/mirrorlist
"#;
        let generated = generate_sandbox_pacman_conf(sample);
        assert!(!generated.contains("Required DatabaseOptional"));
        assert!(!generated.contains("PackageRequired"));
        assert!(generated.contains("SigLevel = Never"));
        assert!(generated.contains("LocalFileSigLevel = Never"));
        assert!(generated.contains("RemoteFileSigLevel = Never"));
    }

    #[test]
    fn test_generate_sandbox_pacman_conf_empty() {
        let generated = generate_sandbox_pacman_conf("");
        assert!(generated.contains("[options]"));
        assert!(generated.contains("SigLevel = Never"));
        assert!(generated.contains("LocalFileSigLevel = Never"));
    }

    #[test]
    fn test_detect_gpg_dir() {
        let sample_default = r#"
[options]
#GPGDir = /etc/pacman.d/gnupg/
"#;
        assert_eq!(detect_gpg_dir(sample_default), PathBuf::from("/etc/pacman.d/gnupg"));

        let sample_custom = r#"
[options]
GPGDir = /custom/pacman/gpg
"#;
        assert_eq!(detect_gpg_dir(sample_custom), PathBuf::from("/custom/pacman/gpg"));
    }
}
