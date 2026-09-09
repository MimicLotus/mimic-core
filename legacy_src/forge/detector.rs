use std::path::Path;

#[derive(Debug, Clone)]
pub struct BuildPlan {
    pub build_type: String,
    pub makedepends: Vec<&'static str>,
    pub build_cmd: String,
    pub install_cmd: String,
}

pub struct BuildDetector;

impl BuildDetector {
    pub fn detect(dir: &Path) -> BuildPlan {
        if dir.join("Cargo.toml").exists() {
            BuildPlan {
                build_type: "Rust / Cargo".to_string(),
                makedepends: vec!["cargo", "rust"],
                build_cmd: "cargo build --release --locked".to_string(),
                install_cmd: r#"find target/release -maxdepth 1 -type f -executable -exec install -Dm755 {} "$FORGE_DEST/bin/" \;"#.to_string(),
            }
        } else if dir.join("meson.build").exists() {
            BuildPlan {
                build_type: "Meson & Ninja".to_string(),
                makedepends: vec!["meson", "ninja"],
                build_cmd: "arch-meson build\nninja -C build".to_string(),
                install_cmd: r#"DESTDIR="$FORGE_DEST" ninja -C build install"#.to_string(),
            }
        } else if dir.join("CMakeLists.txt").exists() {
            BuildPlan {
                build_type: "CMake".to_string(),
                makedepends: vec!["cmake", "gcc"],
                build_cmd: "cmake -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr\ncmake --build build".to_string(),
                install_cmd: r#"DESTDIR="$FORGE_DEST" cmake --install build"#.to_string(),
            }
        } else if dir.join("go.mod").exists() {
            BuildPlan {
                build_type: "Go".to_string(),
                makedepends: vec!["go"],
                build_cmd: r#"go build -v -o app_bin"#.to_string(),
                install_cmd: r#"install -Dm755 app_bin "$FORGE_DEST/bin/app_bin""#.to_string(),
            }
        } else if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
            BuildPlan {
                build_type: "Python".to_string(),
                makedepends: vec!["python-build", "python-installer", "python-wheel", "python-setuptools"],
                build_cmd: "python -m build --wheel --no-isolation".to_string(),
                install_cmd: r#"python -m installer --destdir="$FORGE_DEST" dist/*.whl"#.to_string(),
            }
        } else if dir.join("Makefile").exists() || dir.join("makefile").exists() {
            BuildPlan {
                build_type: "GNU Make".to_string(),
                makedepends: vec!["make", "gcc"],
                build_cmd: "make".to_string(),
                install_cmd: r#"make DESTDIR="$FORGE_DEST" install 2>/dev/null || find . -maxdepth 1 -type f -executable -exec install -Dm755 {} "$FORGE_DEST/bin/" \;"#.to_string(),
            }
        } else {
            BuildPlan {
                build_type: "Generic Executable".to_string(),
                makedepends: vec!["base-devel"],
                build_cmd: "true".to_string(),
                install_cmd: r#"find . -maxdepth 1 -type f -executable -exec install -Dm755 {} "$FORGE_DEST/bin/" \;"#.to_string(),
            }
        }
    }
}
