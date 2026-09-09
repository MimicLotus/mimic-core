use std::io::{stdout, IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alpm::{Alpm, AnyDownloadEvent, AnyEvent, DownloadEvent, DownloadResult, Event, Progress};
use colored::*;

pub const MIMIC_GREEN: &str = "\x1b[38;5;48m";
pub const MIMIC_GREEN_BOLD: &str = "\x1b[1;38;5;48m";
pub const MIMIC_CYAN: &str = "\x1b[38;5;51m";
pub const MIMIC_DIM: &str = "\x1b[38;5;242m";
pub const MIMIC_RESET: &str = "\x1b[0m";

/// Returns true if stdout is connected to an interactive terminal that supports ANSI colors
pub fn is_interactive_tty() -> bool {
    if !stdout().is_terminal() {
        return false;
    }
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }
    true
}

#[derive(Debug)]
struct DownloadTracker {
    filename: String,
    start_time: Instant,
    last_render: Instant,
    downloaded: i64,
    total: i64,
}

pub struct AlpmUiState {
    pub is_tty: bool,
    pub tick: u64,
    current_dl: Option<DownloadTracker>,
    last_pkg_name: Option<String>,
}

impl AlpmUiState {
    pub fn new() -> Self {
        Self {
            is_tty: is_interactive_tty(),
            tick: 0,
            current_dl: None,
            last_pkg_name: None,
        }
    }

    /// Handles download events from ALPM dlcb
    pub fn handle_dl(&mut self, filename: &str, event: AnyDownloadEvent) {
        match event.event() {
            DownloadEvent::Init(_init) => {
                self.current_dl = Some(DownloadTracker {
                    filename: filename.to_string(),
                    start_time: Instant::now(),
                    last_render: Instant::now(),
                    downloaded: 0,
                    total: 0,
                });

                if !self.is_tty {
                    println!("[download] {}: started", filename);
                }
            }
            DownloadEvent::Progress(prog) => {
                let should_render = if let Some(dl) = &mut self.current_dl {
                    dl.downloaded = prog.downloaded;
                    dl.total = prog.total;
                    let now = Instant::now();
                    // Throttle updates to ~40fps (every 25ms) or 100% completion to avoid CPU churn
                    if now.duration_since(dl.last_render) >= Duration::from_millis(25) || prog.downloaded == prog.total {
                        dl.last_render = now;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                if should_render {
                    self.tick = self.tick.wrapping_add(1);
                    if self.is_tty {
                        if let Some(dl) = &self.current_dl {
                            self.render_dl_tty(dl);
                        }
                    }
                }
            }
            DownloadEvent::Completed(comp) => {
                let total_size = format_bytes(comp.total as u64);
                let target_file = self.current_dl.as_ref()
                    .map(|d| d.filename.clone())
                    .unwrap_or_else(|| filename.to_string());

                if self.is_tty {
                    match comp.result {
                        DownloadResult::Success => {
                            println!(
                                "\r\x1b[2K  {} {} ({}) {}",
                                "✔".bold().green(),
                                target_file.bold().white(),
                                total_size.dimmed(),
                                "\x1b[1;38;5;48m[Downloaded]\x1b[0m"
                            );
                        }
                        DownloadResult::UpToDate => {
                            println!(
                                "\r\x1b[2K  {} {} ({}) {}",
                                "✔".bold().cyan(),
                                target_file.bold().white(),
                                total_size.dimmed(),
                                "\x1b[38;5;51m[Up to date]\x1b[0m"
                            );
                        }
                        DownloadResult::Failed => {
                            println!(
                                "\r\x1b[2K  {} {} {}",
                                "✖".bold().red(),
                                target_file.bold().white(),
                                "\x1b[1;31m[Download Failed]\x1b[0m"
                            );
                        }
                    }
                    let _ = stdout().flush();
                } else {
                    match comp.result {
                        DownloadResult::Success => println!("[download] {}: completed ({})", target_file, total_size),
                        DownloadResult::UpToDate => println!("[download] {}: up to date", target_file),
                        DownloadResult::Failed => println!("[download] {}: failed", target_file),
                    }
                }

                self.current_dl = None;
            }
            DownloadEvent::Retry(retry) => {
                if self.is_tty {
                    println!("\r\x1b[2K  {} Retrying download for {} (resume: {})...", "⚠️".yellow(), filename, retry.resume);
                    let _ = stdout().flush();
                } else {
                    println!("[download] {}: retrying (resume={})", filename, retry.resume);
                }
            }
        }
    }

    /// Handles extraction and package transaction progress from ALPM progresscb
    pub fn handle_progress(
        &mut self,
        progress: Progress,
        pkgname: &str,
        percent: i32,
        howmany: usize,
        current: usize,
    ) {
        let (action, action_done) = match progress {
            Progress::AddStart => ("Extracting", "Extracted"),
            Progress::UpgradeStart => ("Upgrading", "Upgraded"),
            Progress::DowngradeStart => ("Downgrading", "Downgraded"),
            Progress::ReinstallStart => ("Reinstalling", "Reinstalled"),
            Progress::RemoveStart => ("Removing", "Removed"),
            Progress::ConflictsStart => ("Checking conflicts", "Conflicts checked"),
            Progress::DiskspaceStart => ("Checking space", "Disk space verified"),
            Progress::IntegrityStart => ("Checking integrity", "Integrity verified"),
            Progress::LoadStart => ("Loading", "Loaded"),
            Progress::KeyringStart => ("Checking keyring", "Keyring verified"),
        };

        if self.is_tty {
            self.tick = self.tick.wrapping_add(1);

            if percent >= 100 {
                // If we previously had a pkg in progress, ensure line is cleanly printed
                println!(
                    "\r\x1b[2K  {} [{}/{}] {} {} {}",
                    "✔".bold().green(),
                    current,
                    howmany,
                    action_done,
                    pkgname.bold().white(),
                    "\x1b[1;38;5;48m[Done]\x1b[0m"
                );
                let _ = stdout().flush();
                self.last_pkg_name = None;
            } else {
                self.render_progress_tty(action, pkgname, percent, howmany, current);
                self.last_pkg_name = Some(pkgname.to_string());
            }
        } else if percent >= 100 {
            println!(
                "[{}] ({}/{}) {}: complete",
                action.to_lowercase().replace(' ', "-"),
                current,
                howmany,
                pkgname
            );
        }
    }

    /// Handles high-level lifecycle events from ALPM eventcb
    pub fn handle_event(&mut self, event: AnyEvent) {
        match event.event() {
            Event::HookRunStart(run) => {
                let desc = run.desc().unwrap_or(run.name());
                if self.is_tty {
                    print!(
                        "\r\x1b[2K  \x1b[1;38;5;220m⚡\x1b[0m \x1b[38;5;242m({}/{})\x1b[0m Running hook: \x1b[1m{}\x1b[0m...",
                        run.position(),
                        run.total(),
                        desc
                    );
                    let _ = stdout().flush();
                } else {
                    println!("[hook] ({}/{}) {}: running...", run.position(), run.total(), desc);
                }
            }
            Event::HookRunDone(run) => {
                let desc = run.desc().unwrap_or(run.name());
                if self.is_tty {
                    println!(
                        "\r\x1b[2K  \x1b[1;38;5;48m✔\x1b[0m \x1b[38;5;242m({}/{})\x1b[0m Completed hook: \x1b[1m{}\x1b[0m \x1b[1;38;5;48m[Done]\x1b[0m",
                        run.position(),
                        run.total(),
                        desc
                    );
                    let _ = stdout().flush();
                } else {
                    println!("[hook] ({}/{}) {}: done", run.position(), run.total(), desc);
                }
            }
            Event::TransactionStart => {
                if self.is_tty {
                    println!("\n  \x1b[1;38;5;48m👑 Initiating transaction commit...\x1b[0m\n");
                } else {
                    println!("[trans] starting transaction commit...");
                }
            }
            Event::TransactionDone => {
                if self.is_tty {
                    println!("\n  \x1b[1;38;5;48m✔ Transaction completed successfully.\x1b[0m\n");
                } else {
                    println!("[trans] transaction completed successfully.");
                }
            }
            Event::FileConflictsStart => {
                if self.is_tty {
                    print!("\r\x1b[2K  \x1b[38;5;51m🔍\x1b[0m Checking for file collisions...");
                    let _ = stdout().flush();
                } else {
                    println!("[trans] checking file conflicts...");
                }
            }
            Event::FileConflictsDone => {
                if self.is_tty {
                    println!("\r\x1b[2K  \x1b[1;38;5;48m✔\x1b[0m File collisions verified.");
                }
            }
            Event::DiskSpaceStart => {
                if self.is_tty {
                    print!("\r\x1b[2K  \x1b[38;5;51m🔍\x1b[0m Checking filesystem disk headroom...");
                    let _ = stdout().flush();
                } else {
                    println!("[trans] checking disk headroom...");
                }
            }
            Event::DiskSpaceDone => {
                if self.is_tty {
                    println!("\r\x1b[2K  \x1b[1;38;5;48m✔\x1b[0m Disk headroom verified.");
                }
            }
            Event::IntegrityStart => {
                if self.is_tty {
                    print!("\r\x1b[2K  \x1b[38;5;51m🔍\x1b[0m Verifying package signatures & integrity...");
                    let _ = stdout().flush();
                } else {
                    println!("[trans] verifying package integrity...");
                }
            }
            Event::IntegrityDone => {
                if self.is_tty {
                    println!("\r\x1b[2K  \x1b[1;38;5;48m✔\x1b[0m Package signatures & integrity verified.");
                }
            }
            _ => {}
        }
    }

    fn render_dl_tty(&self, dl: &DownloadTracker) {
        let elapsed = dl.start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.05 {
            dl.downloaded as f64 / elapsed
        } else {
            0.0
        };

        let pct = if dl.total > 0 {
            ((dl.downloaded * 100) / dl.total).clamp(0, 100) as i32
        } else {
            0
        };

        let dl_str = format_bytes(dl.downloaded as u64);
        let tot_str = if dl.total > 0 {
            format_bytes(dl.total as u64)
        } else {
            "?".to_string()
        };

        let bar = build_candy_bar(pct, 20, self.tick);
        let short_name = if dl.filename.len() > 22 {
            format!("...{}", &dl.filename[dl.filename.len() - 19..])
        } else {
            dl.filename.clone()
        };

        print!(
            "\r\x1b[2K  \x1b[1;38;5;48m📥\x1b[0m \x1b[1m{:<22}\x1b[0m {} \x1b[1;38;5;48m{:>3}%\x1b[0m \x1b[38;5;242m({} / {})\x1b[0m \x1b[38;5;51m{}\x1b[0m",
            short_name,
            bar,
            pct,
            dl_str,
            tot_str,
            format_speed(speed)
        );
        let _ = stdout().flush();
    }

    fn render_progress_tty(
        &self,
        action: &str,
        pkgname: &str,
        percent: i32,
        howmany: usize,
        current: usize,
    ) {
        let bar = build_candy_bar(percent, 20, self.tick);
        let short_pkg = if pkgname.len() > 18 {
            format!("...{}", &pkgname[pkgname.len() - 15..])
        } else {
            pkgname.to_string()
        };

        print!(
            "\r\x1b[2K  \x1b[1;38;5;48m📦\x1b[0m \x1b[38;5;242m[{:>2}/{:<2}]\x1b[0m \x1b[1m{:<11}\x1b[0m \x1b[1;36m{:<18}\x1b[0m {} \x1b[1;38;5;48m{:>3}%\x1b[0m",
            current,
            howmany,
            action,
            short_pkg,
            bar,
            percent.clamp(0, 100)
        );
        let _ = stdout().flush();
    }
}

/// Builds an animated progress bar with the Mimic green theme (\x1b[38;5;48m),
/// a lead cursor crown (👑), and Pac-style candy dots along the unconsumed track.
pub fn build_candy_bar(percent: i32, width: usize, tick: u64) -> String {
    let pct = percent.clamp(0, 100) as usize;
    let filled = (pct * width) / 100;

    if pct >= 100 {
        format!(
            "\x1b[38;5;240m[\x1b[1;38;5;48m{}\x1b[0m 👑\x1b[38;5;240m]\x1b[0m",
            "━".repeat(width)
        )
    } else {
        let fill_str = "━".repeat(filled);
        let remaining = width.saturating_sub(filled + 1);

        let mut track_dots = String::with_capacity(remaining * 16);
        for i in 0..remaining {
            // Smooth tick conveyor transition for candy dots
            let is_candy = (i + (tick as usize / 2)) % 2 == 0;
            if is_candy {
                track_dots.push_str("\x1b[38;5;51m•\x1b[0m");
            } else {
                track_dots.push_str("\x1b[38;5;242m·\x1b[0m");
            }
        }

        format!(
            "\x1b[38;5;240m[\x1b[1;38;5;48m{}\x1b[0m👑{}\x1b[38;5;240m]\x1b[0m",
            fill_str,
            track_dots
        )
    }
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1024.0 * 1024.0 {
        format!("{:.1} MiB/s", bytes_per_sec / (1024.0 * 1024.0))
    } else if bytes_per_sec >= 1024.0 {
        format!("{:.1} KiB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// Attach download, extraction progress, and event callbacks to the ALPM handle
pub fn attach_alpm_callbacks(handle: &Alpm) {
    let state = Arc::new(Mutex::new(AlpmUiState::new()));

    // 1. Download progress callback
    let dl_state = state.clone();
    handle.set_dl_cb(dl_state, |filename, event, state| {
        if let Ok(mut s) = state.lock() {
            s.handle_dl(filename, event);
        }
    });

    // 2. Package extraction & transaction progress callback
    let prog_state = state.clone();
    handle.set_progress_cb(
        prog_state,
        |progress, pkgname, percent, howmany, current, state| {
            if let Ok(mut s) = state.lock() {
                s.handle_progress(progress, pkgname, percent, howmany, current);
            }
        },
    );

    // 3. Lifecycle & hook events callback
    let ev_state = state.clone();
    handle.set_event_cb(ev_state, |event, state| {
        if let Ok(mut s) = state.lock() {
            s.handle_event(event);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GiB");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(500.0), "500 B/s");
        assert_eq!(format_speed(1536.0), "1.5 KiB/s");
        assert_eq!(format_speed(5.5 * 1024.0 * 1024.0), "5.5 MiB/s");
    }

    #[test]
    fn test_candy_bar_lead_cursor() {
        let bar_0 = build_candy_bar(0, 20, 0);
        assert!(bar_0.contains("👑"), "Bar at 0% should contain the crown cursor");
        assert!(bar_0.contains(MIMIC_GREEN_BOLD));

        let bar_50 = build_candy_bar(50, 20, 2);
        assert!(bar_50.contains("👑"), "Bar at 50% should contain the crown cursor");
        assert!(bar_50.contains("━"));

        let bar_100 = build_candy_bar(100, 20, 0);
        assert!(bar_100.contains("👑"));
        assert!(bar_100.contains(&"━".repeat(20)));
    }

    #[test]
    fn test_candy_animation_ticks() {
        let bar_tick0 = build_candy_bar(20, 20, 0);
        let bar_tick1 = build_candy_bar(20, 20, 2);
        // Ticks change the candy dot phases
        assert_ne!(bar_tick0, bar_tick1, "Different ticks should animate the candy dots");
    }

    #[test]
    fn test_non_interactive_state() {
        let mut state = AlpmUiState::new();
        state.is_tty = false; // Force non-interactive mode
        assert!(!state.is_tty);
    }
}
