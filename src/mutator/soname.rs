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
        let host_lib_dirs = ["/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/local/lib"];
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
}
