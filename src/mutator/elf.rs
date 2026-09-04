use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};

use super::soname::{DynamicDependencyAnalysis, SonameScanner};

pub struct ElfMutator;

impl ElfMutator {
    pub fn mutate_binary(
        binary_path: &Path,
        package_id: &str,
        companion_dir: Option<&Path>,
    ) -> Result<DynamicDependencyAnalysis> {
        let analysis = SonameScanner::analyze(binary_path, companion_dir)?;

        // Find patchelf in PATH or ~/.local/bin
        let patchelf_cmd = which::which("patchelf")
            .unwrap_or_else(|_| PathBuf::from("/home/voidlotus/.local/bin/patchelf"));

        if !patchelf_cmd.exists() {
            anyhow::bail!("patchelf binary not found in PATH or ~/.local/bin");
        }

        // 1. Construct the RPATH
        // Priority:
        //  1. $ORIGIN/../lib/<package_id> (Companion Pocket)
        //  2. $ORIGIN/lib (Nested libs)
        //  3. /mimic/lib (Global absorbed organs)
        //  4. /usr/lib (Host base fallback)
        //  5. /usr/lib64
        let new_rpath = format!(
            "$ORIGIN/../lib/{}:$ORIGIN/lib:/mimic/lib:/usr/lib:/usr/lib64",
            package_id
        );

        let status = Command::new(&patchelf_cmd)
            .args(["--set-rpath", &new_rpath, binary_path.to_str().unwrap()])
            .status()
            .with_context(|| format!("Failed to execute patchelf --set-rpath on {:?}", binary_path))?;

        if !status.success() {
            anyhow::bail!("patchelf --set-rpath failed for {:?}", binary_path);
        }

        // 2. Normalize Dynamic Linker Interpreter if required
        // On x86_64, standard glibc loader is /lib64/ld-linux-x86-64.so.2
        if let Some(ref interp) = analysis.interpreter {
            if interp.starts_with("/lib/") || interp.starts_with("/usr/lib/") {
                let host_interp = "/lib64/ld-linux-x86-64.so.2";
                if Path::new(host_interp).exists() && interp != host_interp {
                    let _ = Command::new(&patchelf_cmd)
                        .args(["--set-interpreter", host_interp, binary_path.to_str().unwrap()])
                        .status();
                }
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(binary_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(binary_path, perms)?;
        }

        Ok(analysis)
    }

    pub fn mutate_library(
        lib_path: &Path,
        package_id: &str,
    ) -> Result<()> {
        let patchelf_cmd = which::which("patchelf")
            .unwrap_or_else(|_| PathBuf::from("/home/voidlotus/.local/bin/patchelf"));

        if !patchelf_cmd.exists() {
            anyhow::bail!("patchelf binary not found in PATH or ~/.local/bin");
        }

        let new_rpath = format!(
            "$ORIGIN:$ORIGIN/../{}:/mimic/lib:/usr/lib:/usr/lib64",
            package_id
        );

        let _ = Command::new(&patchelf_cmd)
            .args(["--set-rpath", &new_rpath, lib_path.to_str().unwrap()])
            .status();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(mut perms) = std::fs::metadata(lib_path).map(|m| m.permissions()) {
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(lib_path, perms);
            }
        }

        Ok(())
    }
}
