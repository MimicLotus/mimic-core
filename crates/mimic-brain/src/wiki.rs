use crate::protocol::BrainResponse;

pub struct ArchWikiAdvisor;

impl ArchWikiAdvisor {
    pub fn explain(package: &str) -> BrainResponse {
        let pkg_lower = package.to_lowercase();
        let tokens: Vec<&str> = pkg_lower
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .filter(|t| !t.is_empty())
            .collect();

        let matches = |keywords: &[&str]| -> bool {
            if keywords.iter().any(|k| *k == pkg_lower) {
                return true;
            }
            tokens.iter().any(|tok| keywords.contains(tok))
        };

        if matches(&["pipewire", "pipewire-pulse", "pipewire-jack"]) {
            BrainResponse::Why {
                package: package.to_string(),
                role: "Next-Generation Multimedia Processing & Routing Subsystem".to_string(),
                summary: "PipeWire handles low-latency audio and video processing, sandboxed application streaming (Flatpak/Wayland), and acts as a drop-in replacement for PulseAudio, JACK, and ALSA.".to_string(),
                key_insights: vec![
                    "Unified buffer sharing between video (Wayland screen capture) and audio".to_string(),
                    "Zero-latency graph model compatible with professional DAW workflows".to_string(),
                    "Enforces per-client access control for hermetic security".to_string(),
                ],
                alternatives: vec!["pulseaudio".to_string(), "jack2".to_string()],
                archwiki_topic: Some("https://wiki.archlinux.org/title/PipeWire".to_string()),
            }
        } else if matches(&["hyprland", "hyprland-git"]) {
            BrainResponse::Why {
                package: package.to_string(),
                role: "Dynamic Tiling Wayland Compositor with Hardware Acceleration".to_string(),
                summary: "Hyprland is a modern wlroots/aquamarine based dynamic tiling Wayland compositor written in C++, featuring fluid animations, dual-stack window tiling, and plugin support.".to_string(),
                key_insights: vec![
                    "GPU-accelerated renderer supporting custom bezier curve transitions".to_string(),
                    "IPC socket protocol allowing deep customization from scripts and bar widgets".to_string(),
                    "Native multi-monitor fractional scaling and tearing protocol support".to_string(),
                ],
                alternatives: vec!["sway".to_string(), "niri".to_string(), "river".to_string()],
                archwiki_topic: Some("https://wiki.archlinux.org/title/Hyprland".to_string()),
            }
        } else if matches(&["bwrap", "bubblewrap"]) {
            BrainResponse::Why {
                package: package.to_string(),
                role: "Unprivileged Sandboxing and Containerization Tool".to_string(),
                summary: "Bubblewrap creates unprivileged sandboxes using Linux user namespaces. It is the underlying isolation technology used by Flatpak, devtools, and Mimic's hermetic build container.".to_string(),
                key_insights: vec![
                    "Constructs isolated filesystem mount tables without requiring root / SUID".to_string(),
                    "Restricts network, IPC, PID, and UTS namespaces for clean hermetic compilation".to_string(),
                    "Zero background daemon overhead compared to Docker or Podman".to_string(),
                ],
                alternatives: vec!["firejail".to_string(), "systemd-nspawn".to_string()],
                archwiki_topic: Some("https://wiki.archlinux.org/title/Bubblewrap".to_string()),
            }
        } else if matches(&["sccache"]) {
            BrainResponse::Why {
                package: package.to_string(),
                role: "Shared Compilation Cache for C, C++, and Rust".to_string(),
                summary: "sccache is a ccache-like compiler cache developed by Mozilla, supporting local disk cache and cloud object stores (S3, GCS, Redis) for C/C++ and Rustc compilation.".to_string(),
                key_insights: vec![
                    "Dramatically accelerates iterative AUR and package rebuilds".to_string(),
                    "Seamlessly wraps gcc, clang, and rustc via RUSTC_WRAPPER".to_string(),
                    "Integrated into Mimic's Hermetic Sandbox by default".to_string(),
                ],
                alternatives: vec!["ccache".to_string()],
                archwiki_topic: Some("https://wiki.archlinux.org/title/Ccache".to_string()),
            }
        } else if matches(&["mold"]) {
            BrainResponse::Why {
                package: package.to_string(),
                role: "High-Performance Modern ELF Linker".to_string(),
                summary: "mold is an ultra-fast alternative to GNU ld and LLVM lld, designed to prevent link-time bottlenecks on multi-core systems.".to_string(),
                key_insights: vec![
                    "Often 2x-4x faster than lld and 10x faster than GNU gold/ld.bfd".to_string(),
                    "Supports modern ELF features, LTO, and split DWARF".to_string(),
                    "Used in Mimic's default build flags via -fuse-ld=mold".to_string(),
                ],
                alternatives: vec!["lld".to_string(), "binutils".to_string()],
                archwiki_topic: Some("https://wiki.archlinux.org/title/Mold".to_string()),
            }
        } else if matches(&["ripgrep", "rg"]) {
            BrainResponse::Why {
                package: package.to_string(),
                role: "Ultra-Fast Line-Oriented Regex Search Tool".to_string(),
                summary: "ripgrep (rg) recursively searches directories for a regex pattern while respecting gitignore rules and skipping binary files by default.".to_string(),
                key_insights: vec![
                    "Written in pure Rust on top of the rust-regex finite state machine".to_string(),
                    "Parallel directory traversal with SIMD acceleration".to_string(),
                ],
                alternatives: vec!["grep".to_string(), "the_silver_searcher".to_string(), "ack".to_string()],
                archwiki_topic: Some("https://wiki.archlinux.org/title/Core_utilities#Search".to_string()),
            }
        } else {
            BrainResponse::Why {
                package: package.to_string(),
                role: "Arch Linux / AUR Ecosystem Package".to_string(),
                summary: format!(
                    "Package '{}' is part of the Arch Linux / AUR ecosystem. Query 'mimic info {}' or the official ArchWiki for component breakdown.",
                    package, package
                ),
                key_insights: vec![
                    "Rolling release lifecycle aligned with upstream development".to_string(),
                    "Managed with native libalpm tracking dependencies, conflicts, and file ownership".to_string(),
                ],
                alternatives: vec![],
                archwiki_topic: Some(format!("https://wiki.archlinux.org/title/Special:Search?search={}", package)),
            }
        }
    }
}
