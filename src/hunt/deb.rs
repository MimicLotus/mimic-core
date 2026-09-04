use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use ar::Archive;
use flate2::read::GzDecoder;
use tar::Archive as TarArchive;
use xz2::read::XzDecoder;
use zstd::stream::read::Decoder as ZstdDecoder;

#[derive(Debug, Clone)]
pub struct DebPackageInfo {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub description: String,
    pub extracted_dir: PathBuf,
}

pub struct DebHunter;

impl DebHunter {
    pub fn extract_deb(deb_path: &Path, destination: &Path) -> Result<DebPackageInfo> {
        let file = File::open(deb_path)
            .with_context(|| format!("Failed to open .deb file at {:?}", deb_path))?;
        let mut archive = Archive::new(file);

        let mut package_name = String::new();
        let mut version = String::new();
        let mut architecture = String::new();
        let mut description = String::new();

        let raw_extract_dir = destination.join("raw");
        std::fs::create_dir_all(&raw_extract_dir)?;

        while let Some(entry_result) = archive.next_entry() {
            let mut entry = entry_result.with_context(|| "Corrupted .deb ar archive entry")?;
            let entry_name = String::from_utf8_lossy(entry.header().identifier()).to_string();
            let clean_name = entry_name.trim();

            if clean_name.starts_with("control.tar") {
                let mut buffer = Vec::new();
                entry.read_to_end(&mut buffer)?;

                if let Ok(info) = Self::parse_control_tar(&buffer, clean_name) {
                    package_name = info.0;
                    version = info.1;
                    architecture = info.2;
                    description = info.3;
                }
            } else if clean_name.starts_with("data.tar") {
                let mut buffer = Vec::new();
                entry.read_to_end(&mut buffer)?;
                Self::extract_data_tar(&buffer, clean_name, &raw_extract_dir)?;
            }
        }

        if package_name.is_empty() {
            let file_stem = deb_path.file_stem().unwrap_or_default().to_string_lossy();
            let parts: Vec<&str> = file_stem.split('_').collect();
            package_name = parts.first().unwrap_or(&"unknown").to_string();
            version = parts.get(1).unwrap_or(&"1.0.0").to_string();
            architecture = parts.get(2).unwrap_or(&"x86_64").to_string();
        }

        Ok(DebPackageInfo {
            name: package_name,
            version,
            architecture,
            description,
            extracted_dir: raw_extract_dir,
        })
    }

    fn extract_data_tar(data: &[u8], filename: &str, out_dir: &Path) -> Result<()> {
        let cursor = std::io::Cursor::new(data);

        if filename.ends_with(".xz") {
            let decoder = XzDecoder::new(cursor);
            let mut tar = TarArchive::new(decoder);
            tar.unpack(out_dir)?;
        } else if filename.ends_with(".gz") {
            let decoder = GzDecoder::new(cursor);
            let mut tar = TarArchive::new(decoder);
            tar.unpack(out_dir)?;
        } else if filename.ends_with(".zst") || filename.ends_with(".zstd") {
            let decoder = ZstdDecoder::new(cursor)?;
            let mut tar = TarArchive::new(decoder);
            tar.unpack(out_dir)?;
        } else if filename.ends_with(".tar") {
            let mut tar = TarArchive::new(cursor);
            tar.unpack(out_dir)?;
        } else {
            anyhow::bail!("Unsupported compression format for data archive: {}", filename);
        }

        Ok(())
    }

    fn parse_control_tar(data: &[u8], filename: &str) -> Result<(String, String, String, String)> {
        let cursor = std::io::Cursor::new(data);
        let mut pkg_name = String::new();
        let mut version = String::new();
        let mut arch = String::new();
        let mut desc = String::new();

        let unpack_control = |mut tar: TarArchive<Box<dyn Read>>| -> Result<(String, String, String, String)> {
            for entry in tar.entries()? {
                let mut e = entry?;
                let path = e.path()?;
                if path.ends_with("control") {
                    let mut text = String::new();
                    e.read_to_string(&mut text)?;

                    for line in text.lines() {
                        if let Some(val) = line.strip_prefix("Package: ") {
                            pkg_name = val.trim().to_string();
                        } else if let Some(val) = line.strip_prefix("Version: ") {
                            version = val.trim().to_string();
                        } else if let Some(val) = line.strip_prefix("Architecture: ") {
                            arch = val.trim().to_string();
                        } else if let Some(val) = line.strip_prefix("Description: ") {
                            desc = val.trim().to_string();
                        }
                    }
                }
            }
            Ok((pkg_name, version, arch, desc))
        };

        if filename.ends_with(".xz") {
            let decoder: Box<dyn Read> = Box::new(XzDecoder::new(cursor));
            unpack_control(TarArchive::new(decoder))
        } else if filename.ends_with(".gz") {
            let decoder: Box<dyn Read> = Box::new(GzDecoder::new(cursor));
            unpack_control(TarArchive::new(decoder))
        } else if filename.ends_with(".zst") {
            let decoder: Box<dyn Read> = Box::new(ZstdDecoder::new(cursor)?);
            unpack_control(TarArchive::new(decoder))
        } else {
            let decoder: Box<dyn Read> = Box::new(cursor);
            unpack_control(TarArchive::new(decoder))
        }
    }
}
