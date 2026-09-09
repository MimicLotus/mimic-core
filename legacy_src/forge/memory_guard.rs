use std::fs;

pub struct MemoryProfile {
    pub total_ram_mb: u64,
    pub avail_ram_mb: u64,
    pub has_zram: bool,
    pub has_swap: bool,
    pub is_heavy_pkg: bool,
    pub use_disk_fallback: bool,
    pub build_dir: String,
    pub safe_threads: usize,
    pub disable_lto: bool,
    pub reason: String,
}

pub struct MemoryGuard;

impl MemoryGuard {
    pub fn assess_build_target(pkg_name: &str) -> MemoryProfile {
        let (total_ram_mb, avail_ram_mb) = Self::read_system_memory();
        let (has_zram, has_swap) = Self::detect_swap_and_zram();
        let is_heavy_pkg = Self::is_heavy_package(pkg_name);
        let nproc = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);

        // 1. ZRAM vs Raw Swap Thread Budgeting
        let per_thread_mb = if has_zram { 1536 } else if has_swap { 1792 } else { 2048 };
        let mut safe_threads = (avail_ram_mb / per_thread_mb) as usize;

        if is_heavy_pkg {
            safe_threads = (avail_ram_mb / 2560) as usize;
        }

        safe_threads = safe_threads.clamp(1, nproc);

        // 2. LTO Cliff Protection: If available RAM is <= 8 GB, disable LTO to prevent 14+ GB linker spikes
        let disable_lto = is_heavy_pkg || avail_ram_mb <= 8192;

        // 3. Storage Location Selector (RAM-disk vs NVMe Disk)
        let use_disk_fallback = is_heavy_pkg || avail_ram_mb < 6144 || (total_ram_mb <= 16384 && avail_ram_mb < (total_ram_mb / 2));

        let reason = if is_heavy_pkg {
            format!("Heavy build profile ('{}' requires multi-GB scratch & link space)", pkg_name)
        } else if avail_ram_mb < 6144 {
            format!("Available RAM is low ({:.1} GB available)", avail_ram_mb as f64 / 1024.0)
        } else if total_ram_mb <= 16384 && avail_ram_mb < (total_ram_mb / 2) {
            format!("Available RAM ({:.1} GB) is under 50% of total physical capacity", avail_ram_mb as f64 / 1024.0)
        } else {
            format!("Sufficient memory ({:.1} GB free in tmpfs)", avail_ram_mb as f64 / 1024.0)
        };

        let build_dir = if use_disk_fallback {
            "/var/tmp/mimic-build".to_string()
        } else {
            "/tmp/mimic-build".to_string()
        };

        MemoryProfile {
            total_ram_mb,
            avail_ram_mb,
            has_zram,
            has_swap,
            is_heavy_pkg,
            use_disk_fallback,
            build_dir,
            safe_threads,
            disable_lto,
            reason,
        }
    }

    fn read_system_memory() -> (u64, u64) {
        let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let mut total_kb = 0u64;
        let mut avail_kb = 0u64;

        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = parse_meminfo_kb(line);
            } else if line.starts_with("MemAvailable:") {
                avail_kb = parse_meminfo_kb(line);
            }
        }

        if total_kb == 0 {
            total_kb = 16 * 1024 * 1024; // 16 GB fallback
        }
        if avail_kb == 0 {
            avail_kb = total_kb / 2;
        }

        (total_kb / 1024, avail_kb / 1024)
    }

    fn detect_swap_and_zram() -> (bool, bool) {
        let swaps = fs::read_to_string("/proc/swaps").unwrap_or_default();
        let mut has_zram = false;
        let mut has_swap = false;

        for line in swaps.lines().skip(1) {
            if line.contains("/dev/zram") || line.contains("zram") {
                has_zram = true;
            } else if !line.trim().is_empty() {
                has_swap = true;
            }
        }

        (has_zram, has_swap)
    }

    fn is_heavy_package(pkg_name: &str) -> bool {
        let lower = pkg_name.to_lowercase();
        const HEAVY_TARGETS: &[&str] = &[
            "chromium", "firefox", "rust", "rust-nightly", "rust-analyzer",
            "llvm", "clang", "gcc", "webkit2gtk", "qt6-webengine", "qt5-webengine",
            "electron", "libreoffice", "unreal-engine", "godot", "blender",
            "linux", "linux-cachyos", "linux-zen", "linux-lts", "ceph", "v8"
        ];

        HEAVY_TARGETS.iter().any(|&heavy| lower.contains(heavy))
    }
}

fn parse_meminfo_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}
