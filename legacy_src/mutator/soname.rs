use std::fs::File;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result};
use object::{Object, ObjectSection};

#[derive(Debug, Clone)]
pub struct DynamicDependencyAnalysis {
    pub needed_libraries: Vec<String>,
    pub missing_libraries: Vec<String>,
    pub interpreter: Option<String>,
    pub runpath: Option<String>,
}

pub struct SonameScanner;

impl SonameScanner {
    pub fn is_elf(path: &Path) -> bool {
        if let Ok(mut f) = File::open(path) {
            use std::io::Read;
            let mut magic = [0u8; 4];
            if f.read_exact(&mut magic).is_ok() {
                return magic == [0x7f, b'E', b'L', b'F'];
            }
        }
        false
    }

    pub fn analyze(binary_path: &Path, companion_dir: Option<&Path>) -> Result<DynamicDependencyAnalysis> {
        let buffer = std::fs::read(binary_path)
            .with_context(|| format!("Failed to read binary at {:?}", binary_path))?;

        let obj = object::File::parse(&*buffer)
            .with_context(|| format!("Failed to parse ELF binary at {:?}", binary_path))?;

        let mut interpreter = None;
        for section in obj.sections() {
            if let Ok(name) = section.name() {
                if name == ".interp" {
                    if let Ok(data) = section.data() {
                        let interp_str = String::from_utf8_lossy(data).trim_matches('\0').to_string();
                        interpreter = Some(interp_str);
                    }
                }
            }
        }

        // Query needed libraries and runpath via patchelf / readelf
        let mut needed = Vec::new();
        let mut runpath = None;

        let patchelf_cmd = which::which("patchelf")
            .unwrap_or_else(|_| std::path::PathBuf::from("/home/voidlotus/.local/bin/patchelf"));

        if patchelf_cmd.exists() {
            if let Ok(output) = Command::new(&patchelf_cmd)
                .args(["--print-needed", binary_path.to_str().unwrap()])
                .output()
            {
                if output.status.success() {
                    let out_str = String::from_utf8_lossy(&output.stdout);
                    for line in out_str.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            needed.push(trimmed.to_string());
                        }
                    }
                }
            }

            if let Ok(output) = Command::new(&patchelf_cmd)
                .args(["--print-rpath", binary_path.to_str().unwrap()])
                .output()
            {
                if output.status.success() {
                    let out_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !out_str.is_empty() {
                        runpath = Some(out_str);
                    }
                }
            }
        }

        // Check availability on host and companion dir
        let mut missing = Vec::new();
        let host_lib_dirs = ["/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/local/lib", "/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu"];
        for lib in &needed {
            let mut found = false;

            if let Some(cd) = companion_dir {
                if cd.join(lib).exists() {
                    found = true;
                }
            }

            if !found {
                for dir in &host_lib_dirs {
                    if Path::new(dir).join(lib).exists() {
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                missing.push(lib.clone());
            }
        }

        Ok(DynamicDependencyAnalysis {
            needed_libraries: needed,
            missing_libraries: missing,
            interpreter,
            runpath,
        })
    }

    pub fn get_needed_libraries(binary_path: &Path) -> Result<Vec<String>> {
        let patchelf_cmd = which::which("patchelf")
            .unwrap_or_else(|_| std::path::PathBuf::from("/home/voidlotus/.local/bin/patchelf"));

        let mut needed = Vec::new();

        if patchelf_cmd.exists() {
            if let Ok(output) = Command::new(&patchelf_cmd)
                .args(["--print-needed", binary_path.to_str().unwrap()])
                .output()
            {
                if output.status.success() {
                    let out_str = String::from_utf8_lossy(&output.stdout);
                    for line in out_str.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            needed.push(trimmed.to_string());
                        }
                    }
                    return Ok(needed);
                }
            }
        }

        Ok(needed)
    }

    pub fn scan_unresolved_dependencies(
        raw_extract_dir: &Path,
        companion_dirs: &[std::path::PathBuf],
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut available_so_names = HashSet::new();
        let mut elf_files = Vec::new();

        // 1. Collect all existing .so filenames inside raw_extract_dir
        collect_elf_and_so_recursive(raw_extract_dir, &mut available_so_names, &mut elf_files);

        // 2. Collect .so filenames from active companion directories
        for cd in companion_dirs {
            if cd.exists() {
                if let Ok(entries) = std::fs::read_dir(cd) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.contains(".so") {
                                available_so_names.insert(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        // 3. Scan all ELF files for DT_NEEDED
        let mut missing_sonames = HashSet::new();
        let host_lib_dirs = ["/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/local/lib", "/usr/lib/x86_64-linux-gnu", "/lib/x86_64-linux-gnu"];

        for elf in elf_files {
            if let Ok(needed) = Self::get_needed_libraries(&elf) {
                for lib in needed {
                    if available_so_names.contains(&lib) {
                        continue;
                    }

                    // Check host directories
                    let mut found_on_host = false;
                    for host_dir in &host_lib_dirs {
                        if Path::new(host_dir).join(&lib).exists() {
                            found_on_host = true;
                            break;
                        }
                    }

                    if !found_on_host {
                        missing_sonames.insert(lib);
                    }
                }
            }
        }

        Ok(missing_sonames)
    }

    pub fn compute_sha256(path: &Path) -> Result<String> {
        use sha2::{Sha256, Digest};
        use std::io::Read;

        let mut file = File::open(path)
            .with_context(|| format!("Failed to open file for hashing at {:?}", path))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let hash = hasher.finalize();
        Ok(format!("{:x}", hash))
    }
}

fn collect_elf_and_so_recursive(
    dir: &Path,
    so_names: &mut std::collections::HashSet<String>,
    elf_files: &mut Vec<std::path::PathBuf>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_elf_and_so_recursive(&path, so_names, elf_files);
            } else if path.is_file() || path.is_symlink() {
                if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
                    if fname.contains(".so") {
                        so_names.insert(fname.to_string());
                    }
                }
                if SonameScanner::is_elf(&path) {
                    elf_files.push(path);
                }
            }
        }
    }
}
