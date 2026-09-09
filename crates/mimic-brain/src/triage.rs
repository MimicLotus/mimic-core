use regex::Regex;
use crate::protocol::BrainResponse;

#[derive(Clone)]
pub struct TriageEngine {
    missing_header_re: Regex,
    pkg_config_re: Regex,
    cmake_pkg_re: Regex,
    werror_re: Regex,
    undefined_ref_re: Regex,
    rust_unresolved_re: Regex,
}

impl TriageEngine {
    pub fn new() -> Self {
        Self {
            missing_header_re: Regex::new(r"fatal error:\s*([a-zA-Z0-9_\-/\.]+):\s*No such file or directory").unwrap(),
            pkg_config_re: Regex::new(r"(?:Package|dependency)\s*['`]?([a-zA-Z0-9_\-\.\+]+)['`]?\s*(?:not found|was not found)").unwrap(),
            cmake_pkg_re: Regex::new(r#"Could not find a package configuration file provided by "([a-zA-Z0-9_\-]+)""#).unwrap(),
            werror_re: Regex::new(r"error:\s*.*\s*\[-Werror(?:=([a-zA-Z0-9_\-]+))?\]").unwrap(),
            undefined_ref_re: Regex::new(r"undefined reference to `([a-zA-Z0-9_]+)'").unwrap(),
            rust_unresolved_re: Regex::new(r"error\[E[0-9]+\]:\s*cannot find (?:type|value|function|module) `([a-zA-Z0-9_]+)`").unwrap(),
        }
    }

    pub fn diagnose(&self, package: &str, log: &str, _compiler: Option<&str>, _flags: Option<&str>) -> BrainResponse {
        // 1. Missing Header Detection
        if let Some(caps) = self.missing_header_re.captures(log) {
            let header = &caps[1];
            let probable_pkg = self.map_header_to_package(header);

            return BrainResponse::Diagnose {
                package: package.to_string(),
                root_cause: format!("Missing C/C++ development header: <{}>", header),
                explanation: format!(
                    "The compiler failed because required C/C++ header '{}' was not found in system include paths (/usr/include).",
                    header
                ),
                suggested_fixes: vec![
                    format!("Install '{}' via 'mimic install {}'", probable_pkg, probable_pkg),
                    format!("Add '{}' to 'makedepends=(...)' in PKGBUILD", probable_pkg),
                    "Verify /usr/include is properly mounted in hermetic sandbox".to_string(),
                ],
                suggested_flags: None,
            };
        }

        // 2. Missing PkgConfig Library
        if let Some(caps) = self.pkg_config_re.captures(log) {
            let pkg_config_name = &caps[1];
            return BrainResponse::Diagnose {
                package: package.to_string(),
                root_cause: format!("Missing pkg-config metadata module: '{}'", pkg_config_name),
                explanation: format!(
                    "The build system searched for '{}.pc' using pkgconf/pkg-config, but the library development files are missing.",
                    pkg_config_name
                ),
                suggested_fixes: vec![
                    format!("Install '{}' or corresponding devel package: 'mimic install {}'", pkg_config_name, pkg_config_name),
                    format!("Add '{}' to makedepends in PKGBUILD", pkg_config_name),
                ],
                suggested_flags: None,
            };
        }

        // 3. Missing CMake Package
        if let Some(caps) = self.cmake_pkg_re.captures(log) {
            let cmake_pkg = &caps[1];
            return BrainResponse::Diagnose {
                package: package.to_string(),
                root_cause: format!("Missing CMake module: '{}Config.cmake'", cmake_pkg),
                explanation: format!(
                    "CMake find_package({}) failed because the package configuration files are not installed in /usr/lib/cmake or /usr/share/cmake.",
                    cmake_pkg
                ),
                suggested_fixes: vec![
                    format!("Install '{}' development package via 'mimic install {}'", cmake_pkg.to_lowercase(), cmake_pkg.to_lowercase()),
                    format!("Add '{}' to makedepends in PKGBUILD", cmake_pkg.to_lowercase()),
                ],
                suggested_flags: None,
            };
        }

        // 4. -Werror Treated as Failure
        if let Some(caps) = self.werror_re.captures(log) {
            let warning_type = caps.get(1).map(|m| m.as_str()).unwrap_or("all");
            return BrainResponse::Diagnose {
                package: package.to_string(),
                root_cause: format!("Compiler warning treated as error (-Werror={})", warning_type),
                explanation: "The codebase was compiled with -Werror, causing newer GCC/Clang compilers with stricter checks to fail the build.".to_string(),
                suggested_fixes: vec![
                    "Append '-Wno-error' to CFLAGS/CXXFLAGS".to_string(),
                    format!("Add 'export CFLAGS=\"$CFLAGS -Wno-error -Wno-{}\"' to build() in PKGBUILD", warning_type),
                ],
                suggested_flags: Some("-Wno-error".to_string()),
            };
        }

        // 5. Undefined Reference / Linker Error
        if let Some(caps) = self.undefined_ref_re.captures(log) {
            let symbol = &caps[1];
            let (lib_flag, note) = if symbol.starts_with("cos") || symbol.starts_with("sin") || symbol.starts_with("pow") {
                ("-lm", "Standard C Math Library")
            } else if symbol.starts_with("pthread_") {
                ("-lpthread", "POSIX Threads Library")
            } else if symbol.starts_with("dlopen") || symbol.starts_with("dlsym") {
                ("-ldl", "Dynamic Linking Library")
            } else {
                ("-Wl,--no-as-needed", "Linker symbol resolution")
            };

            return BrainResponse::Diagnose {
                package: package.to_string(),
                root_cause: format!("Unresolved linker symbol: '{}' ({})", symbol, note),
                explanation: format!("The linker (mold/ld) failed to find the definition for '{}' during linking stage.", symbol),
                suggested_fixes: vec![
                    format!("Append '{}' to LDFLAGS", lib_flag),
                    "If building with LTO, try disabling -flto for this package".to_string(),
                ],
                suggested_flags: Some(lib_flag.to_string()),
            };
        }

        // 6. Rust Unresolved Symbol
        if let Some(caps) = self.rust_unresolved_re.captures(log) {
            let rust_sym = &caps[1];
            return BrainResponse::Diagnose {
                package: package.to_string(),
                root_cause: format!("Rust compilation error: unresolved identifier '{}'", rust_sym),
                explanation: "The Rust crate failed to compile due to missing dependencies or rustc version incompatibility.".to_string(),
                suggested_fixes: vec![
                    "Ensure rust/cargo toolchain is up to date via 'mimic install rust cargo'".to_string(),
                    "Check if crate requires a newer MSRV (Minimum Supported Rust Version)".to_string(),
                ],
                suggested_flags: None,
            };
        }

        // Generic Fallback
        BrainResponse::Diagnose {
            package: package.to_string(),
            root_cause: "Unrecognized compilation or script failure".to_string(),
            explanation: "The build failed during execution of makepkg. Review the build logs above for details.".to_string(),
            suggested_fixes: vec![
                "Check the upstream repository issue tracker for known build issues".to_string(),
                "Try building without aggressive compiler flags (e.g. without LTO or -march=native)".to_string(),
                "Inspect the PKGBUILD prepare() and build() functions".to_string(),
            ],
            suggested_flags: None,
        }
    }

    fn map_header_to_package(&self, header: &str) -> &'static str {
        match header {
            h if h.contains("openssl/") => "openssl",
            h if h.contains("wayland-") => "wayland",
            h if h.contains("vulkan/") => "vulkan-headers",
            h if h.contains("X11/") => "libx11",
            h if h.contains("zlib.h") => "zlib",
            h if h.contains("glib.h") || h.contains("glib/") => "glib2",
            h if h.contains("curl/") => "curl",
            h if h.contains("sqlite3.h") => "sqlite",
            h if h.contains("alsa/") => "alsa-lib",
            h if h.contains("pulse/") => "libpulse",
            h if h.contains("pipewire/") => "pipewire",
            h if h.contains("systemd/") => "systemd-libs",
            h if h.contains("pcre2.h") => "pcre2",
            h if h.contains("ffi.h") => "libffi",
            h if h.contains("zstd.h") => "zstd",
            _ => "base-devel",
        }
    }
}
