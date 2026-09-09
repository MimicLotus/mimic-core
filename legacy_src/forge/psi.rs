use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep};
use std::time::Duration;
use colored::*;

pub struct PsiWatcher;

impl PsiWatcher {
    pub fn start(
        pgid: libc::pid_t,
        cancel_signal: Arc<AtomicBool>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut is_frozen = false;

            while !cancel_signal.load(Ordering::Relaxed) {
                if let Ok(content) = fs::read_to_string("/proc/pressure/memory") {
                    let mut pressure_high = false;

                    for line in content.lines() {
                        if line.starts_with("some ") {
                            if let Some(avg10_str) = line.split_whitespace().find(|s| s.starts_with("avg10=")) {
                                if let Ok(val) = avg10_str.trim_start_matches("avg10=").parse::<f64>() {
                                    if val > 20.0 {
                                        pressure_high = true;
                                    }
                                }
                            }
                        }
                    }

                    if pressure_high && !is_frozen {
                        // Freeze entire compiler process group with SIGSTOP
                        unsafe { libc::kill(-pgid, libc::SIGSTOP); }
                        is_frozen = true;
                        eprintln!(
                            "\n{} High Memory Pressure detected (PSI avg10 > 20%). Freezing build process group...",
                            "⚠️".yellow().bold()
                        );
                    } else if !pressure_high && is_frozen {
                        // Pressure dropped; resume compiler workers with SIGCONT
                        unsafe { libc::kill(-pgid, libc::SIGCONT); }
                        is_frozen = false;
                        eprintln!(
                            "\n{} Memory pressure stabilized. Resuming build process group...",
                            "✅".green().bold()
                        );
                    }
                }

                let poll_time = if is_frozen { 1000 } else { 2000 };
                sleep(Duration::from_millis(poll_time));
            }

            // Cleanup: Guarantee workers are resumed before thread termination
            if is_frozen {
                unsafe { libc::kill(-pgid, libc::SIGCONT); }
            }
        })
    }
}
