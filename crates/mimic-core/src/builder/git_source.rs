use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{bail, Context, Result};
use colored::*;

use super::sandbox::BubblewrapSandbox;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildToolchain {
    Rust,
    Meson,
    CMake,
    Make,
}

pub struct GitSourceRunner {
    base_build_dir: PathBuf,
    staging_dir: PathBuf,
    pkg_cache_dir: PathBuf,
    safe_jobs: usize,
}

impl GitSourceRunner {
    pub fn new() -> Self {
        let base_build_dir = PathBuf::from("/var/tmp/mimic-build/git");
        let staging_dir = PathBuf::from("/var/tmp/mimic-build/staging");
        let pkg_cache_dir = resolve_pkg_cache_dir();
        let safe_jobs = super::memory_guard::MemoryGuard::assess().safe_jobs;

        Self {
            base_build_dir,
            staging_dir,
            pkg_cache_dir,
            safe_jobs,
        }
    }

    /// Clone, inspect build system, compile inside bwrap sandbox, stage, and package into .pkg.tar.zst
    pub fn build_from_git(&self, url: &str, branch: Option<&str>) -> Result<PathBuf> {
        let pkg_name = extract_repo_name(url)?;
        println!("{} Processing Git source package: '{}'...", "::".cyan().bold(), pkg_name.bold());

        let repo_dir = self.base_build_dir.join(&pkg_name);
        if repo_dir.exists() {
            let _ = std::fs::remove_dir_all(&repo_dir);
        }
        std::fs::create_dir_all(&self.base_build_dir)?;

        let pkg_staging = self.staging_dir.join(&pkg_name);
        if pkg_staging.exists() {
            let _ = std::fs::remove_dir_all(&pkg_staging);
        }
        std::fs::create_dir_all(&pkg_staging)?;

        // 1. Clone repository
        println!("{} Cloning repository from '{}'...", "::".cyan().bold(), url.bold());
        let mut git_clone = Command::new("git");
        git_clone.args(["clone", "--depth", "1"]);
        if let Some(b) = branch {
            git_clone.args(["--branch", b]);
        }
        git_clone.args([url, repo_dir.to_str().unwrap()]);

        let clone_status = git_clone.status()
            .with_context(|| format!("Failed to clone git repository '{}'", url))?;
        if !clone_status.success() {
            bail!("git clone exited with error code: {}", clone_status);
        }

        // 2. Query version and commit info
        let version = self.query_git_version(&repo_dir);
        println!("  • Version tag resolved: {}", version.bold().green());

        // 3. Detect build toolchain
        let (toolchain, build_script) = self.detect_toolchain(&repo_dir)?;
        let toolchain_name = match toolchain {
            BuildToolchain::Rust => "Rust (cargo)",
            BuildToolchain::Meson => "Meson / Ninja",
            BuildToolchain::CMake => "CMake / Ninja",
            BuildToolchain::Make => "GNU Make",
        };
        println!("  • Toolchain detected:   {}", toolchain_name.bold().yellow());

        // 4. Run Sandbox Execution
        let sandbox = BubblewrapSandbox::new(&pkg_name, &repo_dir);
        let extra_binds = [
            (pkg_staging.as_path(), "/staging"),
        ];
        let extra_envs = [
            ("DESTDIR", "/staging"),
            ("PREFIX", "/usr"),
            ("INSTALL_PREFIX", "/usr"),
        ];

        println!("\n{} Executing hermetic build and staging...", "::".green().bold());
        sandbox.execute_script(&build_script, &extra_binds, &extra_envs)?;

        // 5. Verify staging contents, fallback copy if needed
        self.ensure_staged_artifacts(&pkg_name, &repo_dir, &pkg_staging, toolchain)?;

        // 6. Generate Arch metadata (.PKGINFO & .BUILDINFO)
        self.generate_pkginfo(&pkg_name, &version, url, &pkg_staging)?;

        // 7. Compress into .pkg.tar.zst
        let pkg_artifact = self.compress_package(&pkg_name, &version, &pkg_staging)?;

        println!("\n{} Successfully generated Git binary package:", "✔".green().bold());
        println!("  📦 {}", pkg_artifact.display().to_string().bold().green());

        Ok(pkg_artifact)
    }

    fn detect_toolchain(&self, repo_dir: &Path) -> Result<(BuildToolchain, String)> {
        let jobs = self.safe_jobs;
        if repo_dir.join("Cargo.toml").exists() {
            let script = format!(
                "mkdir -p /staging/usr/bin && cargo build --release --locked -j {} && \
                 find target/release -maxdepth 1 -type f -executable ! -name '*.so' ! -name '*.d' -exec cp {{}} /staging/usr/bin/ \\;",
                jobs
            );
            Ok((BuildToolchain::Rust, script))
        } else if repo_dir.join("meson.build").exists() {
            let script = format!(
                "meson setup _build --prefix=/usr --buildtype=release && \
                 ninja -C _build -j {} && \
                 DESTDIR=/staging ninja -C _build -j {} install",
                jobs, jobs
            );
            Ok((BuildToolchain::Meson, script))
        } else if repo_dir.join("src/meson.build").exists() {
            let script = format!(
                "meson setup _build src --prefix=/usr --buildtype=release && \
                 ninja -C _build -j {} && \
                 DESTDIR=/staging ninja -C _build -j {} install",
                jobs, jobs
            );
            Ok((BuildToolchain::Meson, script))
        } else if repo_dir.join("CMakeLists.txt").exists() {
            let script = format!(
                "cmake -B _build -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_BUILD_TYPE=Release && \
                 cmake --build _build --parallel {} && \
                 DESTDIR=/staging cmake --install _build",
                jobs
            );
            Ok((BuildToolchain::CMake, script))
        } else if repo_dir.join("src/CMakeLists.txt").exists() {
            let script = format!(
                "cmake -B _build -S src -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_BUILD_TYPE=Release && \
                 cmake --build _build --parallel {} && \
                 DESTDIR=/staging cmake --install _build",
                jobs
            );
            Ok((BuildToolchain::CMake, script))
        } else if repo_dir.join("Makefile").exists() || repo_dir.join("makefile").exists() {
            let script = format!(
                "make -j{} && (make DESTDIR=/staging PREFIX=/usr install || make DESTDIR=/staging install || make PREFIX=/staging/usr install)",
                jobs
            );
            Ok((BuildToolchain::Make, script))
        } else if repo_dir.join("src/Makefile").exists() || repo_dir.join("src/makefile").exists() {
            let script = format!(
                "make -C src -j{} && (make -C src DESTDIR=/staging PREFIX=/usr install || make -C src DESTDIR=/staging install || make -C src PREFIX=/staging/usr install)",
                jobs
            );
            Ok((BuildToolchain::Make, script))
        } else {
            bail!("Could not automatically detect build system (Cargo.toml, meson.build, CMakeLists.txt, or Makefile).");
        }
    }

    fn query_git_version(&self, repo_dir: &Path) -> String {
        // Try git describe --tags
        if let Ok(output) = Command::new("git").args(["describe", "--tags", "--always"]).current_dir(repo_dir).output() {
            if output.status.success() {
                let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !tag.is_empty() {
                    let sanitized = tag.trim_start_matches('v').replace('-', ".");
                    let count = self.query_commit_count(repo_dir);
                    return format!("{}.r{}-1", sanitized, count);
                }
            }
        }

        // Fallback to 0.1.0.r<count>.<sha>-1
        let count = self.query_commit_count(repo_dir);
        let sha = if let Ok(output) = Command::new("git").args(["rev-parse", "--short", "HEAD"]).current_dir(repo_dir).output() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            "g000000".to_string()
        };

        format!("0.1.0.r{}.g{}-1", count, sha)
    }

    fn query_commit_count(&self, repo_dir: &Path) -> usize {
        if let Ok(output) = Command::new("git").args(["rev-list", "--count", "HEAD"]).current_dir(repo_dir).output() {
            if output.status.success() {
                if let Ok(cnt) = String::from_utf8_lossy(&output.stdout).trim().parse::<usize>() {
                    return cnt;
                }
            }
        }
        1
    }

    fn ensure_staged_artifacts(&self, pkg_name: &str, repo_dir: &Path, staging_dir: &Path, toolchain: BuildToolchain) -> Result<()> {
        let has_files = count_files_in_dir(staging_dir) > 0;
        if has_files {
            return Ok(());
        }

        // Fallback: If staging is empty, look for compiled binaries in build artifacts
        let bin_dest = staging_dir.join("usr/bin");
        std::fs::create_dir_all(&bin_dest)?;

        match toolchain {
            BuildToolchain::Rust => {
                let release_dir = repo_dir.join("target/release");
                if release_dir.exists() {
                    for entry in std::fs::read_dir(release_dir)? {
                        let entry = entry?;
                        let p = entry.path();
                        if p.is_file() && is_executable(&p) {
                            let fname = p.file_name().unwrap().to_str().unwrap();
                            if !fname.contains('.') {
                                std::fs::copy(&p, bin_dest.join(fname))?;
                            }
                        }
                    }
                }
            }
            BuildToolchain::CMake | BuildToolchain::Meson => {
                let build_dir = repo_dir.join("_build");
                if build_dir.exists() {
                    for entry in walk_executable_files(&build_dir) {
                        let fname = entry.file_name().unwrap().to_str().unwrap();
                        if !fname.contains('.') {
                            std::fs::copy(&entry, bin_dest.join(fname))?;
                        }
                    }
                }
            }
            BuildToolchain::Make => {
                for entry in walk_executable_files(repo_dir) {
                    let fname = entry.file_name().unwrap().to_str().unwrap();
                    if fname == pkg_name {
                        std::fs::copy(&entry, bin_dest.join(fname))?;
                    }
                }
            }
        }

        if count_files_in_dir(staging_dir) == 0 {
            bail!("Build completed, but no installation artifacts were found in $STAGING or build directory.");
        }

        Ok(())
    }

    fn generate_pkginfo(&self, pkg_name: &str, version: &str, url: &str, staging_dir: &Path) -> Result<()> {
        let total_size = compute_dir_size(staging_dir);
        let build_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let pkginfo = format!(
            "# Generated by Mimic v4.0.0 Git Source Runner\n\
             pkgname = {}\n\
             pkgver = {}\n\
             pkgdesc = {} built from git source ({})\n\
             url = {}\n\
             builddate = {}\n\
             packager = Mimic Native Git Builder <mimic@archlinux>\n\
             size = {}\n\
             arch = x86_64\n\
             license = custom\n",
            pkg_name, version, pkg_name, url, url, build_timestamp, total_size
        );

        std::fs::write(staging_dir.join(".PKGINFO"), pkginfo)
            .context("Failed to write .PKGINFO metadata")?;

        let buildinfo = format!(
            "format = 2\n\
             pkgname = {}\n\
             pkgver = {}\n\
             pkgarch = x86_64\n\
             pkgbuild_sha256sum = none\n\
             packager = Mimic Native Git Builder <mimic@archlinux>\n\
             builddate = {}\n\
             builddir = /build\n\
             buildenv = sccache mold\n\
             installed = base\n",
            pkg_name, version, build_timestamp
        );

        std::fs::write(staging_dir.join(".BUILDINFO"), buildinfo)
            .context("Failed to write .BUILDINFO metadata")?;

        Ok(())
    }

    fn compress_package(&self, pkg_name: &str, version: &str, staging_dir: &Path) -> Result<PathBuf> {
        let pkg_filename = format!("{}-{}-x86_64.pkg.tar.zst", pkg_name, version);
        let _ = std::fs::create_dir_all(&self.pkg_cache_dir);
        let output_pkg_path = self.pkg_cache_dir.join(&pkg_filename);

        println!("{} Compressing package archive to '{}'...", "::".cyan().bold(), output_pkg_path.display().to_string().cyan());

        let bsdtar_bin = which::which("bsdtar").unwrap_or_else(|_| PathBuf::from("/usr/bin/bsdtar"));
        if bsdtar_bin.exists() {
            let mut cmd = Command::new(&bsdtar_bin);
            cmd.args(["--uid", "0", "--gid", "0", "--zstd", "-cf", output_pkg_path.to_str().unwrap()]);
            cmd.arg("-C").arg(staging_dir.to_str().unwrap());
            cmd.args([".PKGINFO", ".BUILDINFO"]);
            
            // Add other top-level entries
            for entry in std::fs::read_dir(staging_dir)? {
                let entry = entry?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str != ".PKGINFO" && name_str != ".BUILDINFO" {
                    cmd.arg(name_str.as_ref());
                }
            }

            let status = cmd.status().with_context(|| "Failed to execute bsdtar for package archival")?;
            if status.success() {
                return Ok(output_pkg_path);
            }
        }

        // Fallback pure-rust compression
        let file = std::fs::File::create(&output_pkg_path)
            .with_context(|| format!("Failed to create output package archive at {:?}", output_pkg_path))?;
        
        let zstd_enc = zstd::stream::Encoder::new(file, 3)
            .context("Failed to initialize Zstandard compression stream")?;
        let mut tar_builder = tar::Builder::new(zstd_enc.auto_finish());

        // First append metadata
        if staging_dir.join(".PKGINFO").exists() {
            let mut f = std::fs::File::open(staging_dir.join(".PKGINFO"))?;
            tar_builder.append_file(".PKGINFO", &mut f)?;
        }
        if staging_dir.join(".BUILDINFO").exists() {
            let mut f = std::fs::File::open(staging_dir.join(".BUILDINFO"))?;
            tar_builder.append_file(".BUILDINFO", &mut f)?;
        }

        // Append rest of filesystem
        for entry in std::fs::read_dir(staging_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == ".PKGINFO" || name_str == ".BUILDINFO" {
                continue;
            }
            if path.is_dir() {
                tar_builder.append_dir_all(&name, &path)?;
            } else {
                let mut f = std::fs::File::open(&path)?;
                tar_builder.append_file(&name, &mut f)?;
            }
        }

        tar_builder.finish()
            .context("Failed to finalize tar.zst archive")?;

        Ok(output_pkg_path)
    }
}

fn extract_repo_name(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some(pos) = trimmed.rfind('/') {
        let name = &trimmed[pos + 1..];
        if !name.is_empty() {
            return Ok(name.to_lowercase());
        }
    }
    bail!("Unable to parse repository name from URL: '{}'", url);
}

fn resolve_pkg_cache_dir() -> PathBuf {
    let sys_cache = PathBuf::from("/var/cache/mimic/pkg");
    if sys_cache.exists() && std::fs::create_dir_all(&sys_cache).is_ok() {
        return sys_cache;
    }
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache/mimic/pkg")
    } else {
        PathBuf::from("/var/tmp/mimic-cache/pkg")
    }
}

fn compute_dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Ok(meta) = p.metadata() {
                    total += meta.len();
                }
            } else if p.is_dir() {
                total += compute_dir_size(&p);
            }
        }
    }
    total
}

fn count_files_in_dir(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !fname.starts_with('.') {
                    count += 1;
                }
            } else if p.is_dir() {
                count += count_files_in_dir(&p);
            }
        }
    }
    count
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata() {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    false
}

fn walk_executable_files(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && is_executable(&p) {
                results.push(p);
            } else if p.is_dir() {
                results.extend(walk_executable_files(&p));
            }
        }
    }
    results
}
