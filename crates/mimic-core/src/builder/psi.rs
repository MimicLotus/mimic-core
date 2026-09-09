use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep, JoinHandle};
use std::time::Duration;
use colored::*;

pub struct PsiWatcher {
    cancel_signal: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pgid: libc::pid_t,
}

impl PsiWatcher {
    pub fn start(pgid: libc::pid_t) -> Self {
        let cancel_signal = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel_signal);

        let handle = thread::spawn(move || {
            let mut is_frozen = false;

            while !cancel_clone.load(Ordering::Relaxed) {
                if let Ok(content) = fs::read_to_string("/proc/pressure/memory") {
                    let mut pressure_high = false;

                    for line in content.lines() {
                        if line.starts_with("some ") {
                            if let Some(avg10_str) = line.split_whitespace().find(|s| s.starts_with("avg10=")) {
                                if let Ok(val) = avg10_str.trim_start_matches("avg10=").parse::<f64>() {
                                    if val > 25.0 {
                                        pressure_high = true;
                                    }
                                }
                            }
                        }
                    }

                    if pressure_high && !is_frozen {
                        // Freeze entire sandbox compiler process group with SIGSTOP
                        unsafe { libc::kill(-pgid, libc::SIGSTOP); }
                        is_frozen = true;
                        eprintln!(
                            "\n{} High Memory Pressure detected (PSI avg10 > 25%). Freezing build process group {}...",
                            "⚠️".yellow().bold(),
                            pgid
                        );
                    } else if !pressure_high && is_frozen {
                        // Pressure dropped; resume compiler workers with SIGCONT
                        unsafe { libc::kill(-pgid, libc::SIGCONT); }
                        is_frozen = false;
                        eprintln!(
                            "\n{} Memory pressure stabilized. Resuming build process group {}...",
                            "✅".green().bold(),
                            pgid
                        );
                    }
                }

                let poll_time = if is_frozen { 1000 } else { 1500 };
                sleep(Duration::from_millis(poll_time));
            }

            // Cleanup: Guarantee workers are resumed before thread terminates
            if is_frozen {
                unsafe { libc::kill(-pgid, libc::SIGCONT); }
            }
        });

        Self {
            cancel_signal,
            handle: Some(handle),
            pgid,
        }
    }

    pub fn stop(mut self) {
        self.cancel_signal.store(true, Ordering::Relaxed);
        unsafe { libc::kill(-self.pgid, libc::SIGCONT); }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PsiWatcher {
    fn drop(&mut self) {
        self.cancel_signal.store(true, Ordering::Relaxed);
        unsafe { libc::kill(-self.pgid, libc::SIGCONT); }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
