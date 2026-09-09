use std::fs;

#[derive(Debug, Clone)]
pub struct MemoryProfile {
    pub total_ram_mib: u64,
    pub safe_jobs: usize,
    pub is_low_ram: bool,
    pub cflags: String,
    pub cxxflags: String,
    pub rustflags: String,
    pub tuning_desc: String,
}

pub struct MemoryGuard;

impl MemoryGuard {
    pub fn assess() -> MemoryProfile {
        let total_ram_mib = Self::read_total_ram_mib();
        let nproc = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Self::assess_for_ram(total_ram_mib, nproc)
    }

    pub fn assess_for_ram(total_ram_mib: u64, nproc: usize) -> MemoryProfile {
        // 1. Safe Concurrency Budgeting:
        // let safe_jobs = (total_ram_mib / 1536).clamp(1, nproc - 1)
        let max_jobs = nproc.saturating_sub(1).max(1);
        let safe_jobs = ((total_ram_mib / 1536) as usize).clamp(1, max_jobs);

        // 2. Adaptive Optimization & LTO Throttling:
        // If RAM <= 8 GiB (8192 MiB): Strip -flto=auto and use -O2 -pipe
        // If RAM > 8 GiB: Allow full -O3 -pipe -flto=auto
        let is_low_ram = total_ram_mib <= 8192;
        let (cflags, cxxflags, rustflags, tuning_desc) = if is_low_ram {
            (
                "-march=native -O2 -pipe -fno-plt".to_string(),
                "-march=native -O2 -pipe -fno-plt".to_string(),
                "-C target-cpu=native -C opt-level=2".to_string(),
                "-march=native -O2 -pipe (LTO disabled for RAM <= 8 GiB)".to_string(),
            )
        } else {
            (
                "-march=native -O3 -pipe -fno-plt -flto=auto".to_string(),
                "-march=native -O3 -pipe -fno-plt -flto=auto".to_string(),
                "-C target-cpu=native -C opt-level=3".to_string(),
                "-march=native -O3 -pipe -flto=auto".to_string(),
            )
        };

        MemoryProfile {
            total_ram_mib,
            safe_jobs,
            is_low_ram,
            cflags,
            cxxflags,
            rustflags,
            tuning_desc,
        }
    }

    pub fn read_total_ram_mib() -> u64 {
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return kb / 1024;
                        }
                    }
                }
            }
        }
        // Fallback default: 8 GiB
        8192
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_guard_low_ram_budgeting() {
        // ~5.6 GiB (5748 MiB) on 12-thread CPU
        let profile = MemoryGuard::assess_for_ram(5748, 12);
        assert_eq!(profile.safe_jobs, 3);
        assert!(profile.is_low_ram);
        assert!(profile.cflags.contains("-O2 -pipe"));
        assert!(!profile.cflags.contains("-flto"));
        assert!(!profile.cxxflags.contains("-flto"));
        assert_eq!(profile.rustflags, "-C target-cpu=native -C opt-level=2");
    }

    #[test]
    fn test_memory_guard_high_ram_budgeting() {
        // 32 GiB (32768 MiB) on 16-thread CPU
        let profile = MemoryGuard::assess_for_ram(32768, 16);
        assert_eq!(profile.safe_jobs, 15); // capped by nproc - 1
        assert!(!profile.is_low_ram);
        assert!(profile.cflags.contains("-O3 -pipe -fno-plt -flto=auto"));
        assert!(profile.cxxflags.contains("-O3 -pipe -fno-plt -flto=auto"));
        assert_eq!(profile.rustflags, "-C target-cpu=native -C opt-level=3");
    }

    #[test]
    fn test_memory_guard_single_core_clamp() {
        let profile = MemoryGuard::assess_for_ram(2048, 1);
        assert_eq!(profile.safe_jobs, 1);
        assert!(profile.is_low_ram);
    }

    #[test]
    fn test_system_ram_read() {
        let ram = MemoryGuard::read_total_ram_mib();
        assert!(ram > 0, "System RAM should be greater than 0");
    }
}
