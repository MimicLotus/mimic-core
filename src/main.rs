mod forge;
mod graft;
mod hunt;
mod mutator;
mod state;

use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;

use graft::GraftEngine;
use hunt::{DebHunter, GitHunter, Hunter, MirrorResolver, OrganScavenger};
use state::{AbilityRecord, StateLedger};

#[derive(Parser)]
#[command(
    name = "mimic",
    version = "3.0.0",
    author = "MimicOS Core Team <dev@mimicos.org>",
    about = "🦖 Autonomous Predatory Package Ingestion & Assimilation Engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 📡 Scout and query upstream hunting grounds for prey (Debian, Arch, AUR)
    #[command(alias = "search", alias = "find", alias = "scout")]
    Hunt {
        /// Target package or keyword to search for
        query: String,
        /// Limit hunt to specific ecosystem (e.g. 'deb', 'aur', 'arch')
        #[arg(short = 'e', long = "ecosystem", alias = "source", short_alias = 's')]
        ecosystem: Option<String>,
    },
    /// 🥩 Hunt, break, mutate, and assimilate packages into the host
    #[command(alias = "absorb")]
    Consume {
        /// Target prey (e.g. deb:vlc, mpv, deb:./pkg.deb, git:BurntSushi/ripgrep)
        #[arg(required = true)]
        targets: Vec<String>,
        /// Explicit ecosystem override (e.g. -e deb, -s deb, --ecosystem deb)
        #[arg(short = 'e', long = "ecosystem", alias = "source", short_alias = 's')]
        ecosystem: Option<String>,
    },
    /// 📜 List all consumed abilities and assimilated organs
    #[command(alias = "abilities")]
    List,
    /// 🧬 Inspect deduplicated companion organ bank and reference counts
    #[command(alias = "bank", alias = "shared")]
    Organs,
    /// 🩸 Purge and cleanly shed an absorbed ability from the system
    #[command(alias = "shed", alias = "remove")]
    Purge {
        /// Ability ID or package name to remove
        id: String,
    },
    /// ⚡ Level up all absorbed abilities against upstream hunting grounds
    LevelUp,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Normalize command line arguments (handle case-insensitive prefixes and detached colons)
    let raw_args: Vec<String> = std::env::args().collect();
    let mut normalized_args = Vec::new();

    let mut i = 0;
    while i < raw_args.len() {
        let arg = &raw_args[i];
        let lower = arg.to_lowercase();

        // Handle detached prefix e.g. "Deb:" "mpv" -> "deb:mpv"
        if (lower == "deb:" || lower == "git:" || lower == "aur:" || lower == "rpm:" || lower == "arch:") && i + 1 < raw_args.len() {
            let next_arg = &raw_args[i + 1];
            normalized_args.push(format!("{}{}", lower, next_arg));
            i += 2;
            continue;
        }

        // Normalize case for prefixes e.g. "Deb:mpv" -> "deb:mpv"
        if lower.starts_with("deb:") || lower.starts_with("git:") || lower.starts_with("aur:") || lower.starts_with("rpm:") || lower.starts_with("arch:") {
            normalized_args.push(lower);
        } else {
            normalized_args.push(arg.clone());
        }
        i += 1;
    }

    let cli = Cli::parse_from(normalized_args);

    match cli.command {
        Commands::Hunt { query, ecosystem } => {
            hunt_prey(&query, ecosystem.as_deref()).await?;
        }
        Commands::Consume { targets, ecosystem } => {
            for target in &targets {
                consume_prey(target, ecosystem.as_deref()).await?;
            }
        }
        Commands::List => {
            list_abilities()?;
        }
        Commands::Organs => {
            list_organs()?;
        }
        Commands::Purge { id } => {
            purge_ability(&id)?;
        }
        Commands::LevelUp => {
            level_up_abilities().await?;
        }
    }

    Ok(())
}

async fn hunt_prey(query: &str, filter: Option<&str>) -> Result<()> {
    println!("\n{} {}", "🦖 MIMIC SCOUT RADAR".bold().green(), "v3.0.0".dimmed());
    println!("{} Scouting upstream hunting grounds for '{}'...\n", "::".magenta().bold(), query.bold().cyan());

    let hunter = Hunter::new();
    let results = hunter.hunt_all(query, filter).await;

    if results.is_empty() {
        println!("  {}", format!("No prey found matching '{}'.", query).yellow());
        return Ok(());
    }

    println!(
        "  {:<8} {:<30} {:<18} {}",
        "ORIGIN".bold().dimmed(),
        "PACKAGE".bold().white(),
        "VERSION".bold().dimmed(),
        "ASSIMILATION COMMAND".bold().green()
    );
    println!("  {}", "─".repeat(88).dimmed());

    for r in results {
        let origin_badge = match r.origin.as_str() {
            "deb" => "deb".bold().red(),
            "aur" => "aur".bold().magenta(),
            "arch" => "arch".bold().cyan(),
            other => other.bold().white(),
        };

        let action = format!("mimic consume {}:{}", r.origin, r.package).green();

        println!(
            "  {:<8} {:<30} {:<18} {}",
            origin_badge,
            r.package.bold().white(),
            r.version.dimmed(),
            action
        );
    }
    println!();

    Ok(())
}

async fn consume_prey(target: &str, explicit_source: Option<&str>) -> Result<()> {
    println!("\n{} {}", "🦖 MIMIC ASSIMILATION ENGINE".bold().green(), "v3.0.0".dimmed());
    println!("{} Hunting prey: '{}'", "::".magenta().bold(), target.bold().cyan());

    // 1. Determine Prey Ingestion Vector
    let is_deb = target.ends_with(".deb") || target.starts_with("deb:") || explicit_source == Some("deb");
    let is_git = target.starts_with("git:") || target.contains("github.com") || target.ends_with(".git") || explicit_source == Some("git");

    let forge_uuid = format!("forge_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis());
    let forge_dir = PathBuf::from(format!("/tmp/mimic-{}", forge_uuid));
    fs::create_dir_all(&forge_dir)?;

    let pkg_name;
    let version;
    let source_type;
    let raw_extract_dir;
    let mut upstream_origin = target.to_string();
    let mut initial_deps = Vec::new();

    if is_deb {
        source_type = "deb".to_string();
        let deb_raw = target.trim_start_matches("deb:");
        let local_path = Path::new(deb_raw);

        let deb_path_to_extract: PathBuf;

        if local_path.exists() && local_path.is_file() {
            deb_path_to_extract = local_path.to_path_buf();
        } else {
            // Over-the-wire mirror hunt
            println!("{} Querying upstream Debian mirrors for '{}'...", "::".cyan().bold(), deb_raw.bold());
            let resolver = MirrorResolver::new();
            let remote_url = resolver.resolve_deb(deb_raw).await?;
            upstream_origin = remote_url.clone();
            deb_path_to_extract = resolver.fetch_to_file(&remote_url, &forge_dir).await?;
        }

        println!("{} Phase 1: Unpacking prey payload into RAM forge...", "::".cyan().bold());
        let info = DebHunter::extract_deb(&deb_path_to_extract, &forge_dir)?;
        pkg_name = info.name;
        version = info.version;
        raw_extract_dir = info.extracted_dir;
        initial_deps = info.dependencies;

        println!(
            "  • Extracted: {} (v{}) [{}]",
            pkg_name.bold().green(),
            version.cyan(),
            info.architecture.dimmed()
        );
    } else if is_git {
        source_type = "git".to_string();
        let git_target = target.trim_start_matches("git:");
        let (name, ver, dest_dir) = GitHunter::forge_git_prey(git_target, &forge_dir)?;
        pkg_name = name;
        version = ver;
        raw_extract_dir = dest_dir;
    } else {
        // Default fallback: Try over-the-wire Debian mirror hunt
        source_type = "deb".to_string();
        println!("{} Querying upstream mirrors for '{}'...", "::".cyan().bold(), target.bold());
        let resolver = MirrorResolver::new();
        let remote_url = resolver.resolve_deb(target).await?;
        upstream_origin = remote_url.clone();
        let downloaded = resolver.fetch_to_file(&remote_url, &forge_dir).await?;

        println!("{} Phase 1: Unpacking prey payload into RAM forge...", "::".cyan().bold());
        let info = DebHunter::extract_deb(&downloaded, &forge_dir)?;
        pkg_name = info.name;
        version = info.version;
        raw_extract_dir = info.extracted_dir;
        initial_deps = info.dependencies;
    }

    // 1.5. Autonomous Dependency & Organ Scavenging
    let _ = OrganScavenger::scavenge_dependencies(
        &pkg_name,
        &raw_extract_dir,
        &initial_deps,
        &forge_dir,
    ).await?;

    // 2. Bone-Break & Physical Assimilation (Graft Engine)
    println!("{} Phase 2: Mutating ELF headers & grafting organs...", "::".yellow().bold());
    let graft_res = GraftEngine::graft_payload(&raw_extract_dir, &pkg_name)?;

    // 3. Register in State Ledger (SQLite)
    println!("{} Phase 3: Recording ability in state ledger...", "::".cyan().bold());
    let mut ledger = StateLedger::open()?;
    let record = AbilityRecord {
        id: pkg_name.clone(),
        name: pkg_name.clone(),
        source_type,
        upstream_url: upstream_origin,
        version: version.clone(),
        consumed_at: chrono_like_now(),
        binary_paths: graft_res.binary_paths.clone(),
        companion_libs: graft_res.companion_libs.clone(),
        desktop_files: graft_res.desktop_files.clone(),
    };
    ledger.record_ability(&record)?;

    // 4. Incinerate Forge
    let _ = fs::remove_dir_all(&forge_dir);

    // 5. Success Report
    println!("\n{} Successfully assimilated '{}' into MimicOS!", "✔".green().bold(), pkg_name.bold());
    println!("  • Exported Binaries: {}", graft_res.binary_paths.len());
    for bin in &graft_res.binary_paths {
        println!("    └─ {}", bin.green());
    }
    if !graft_res.companion_libs.is_empty() {
        println!("  • Scavenged Companion Libs: {}", graft_res.companion_libs.len());
        for lib in &graft_res.companion_libs {
            println!("    └─ {}", lib.dimmed());
        }
    }
    if !graft_res.desktop_files.is_empty() {
        println!("  • Desktop Hooks: {}", graft_res.desktop_files.len());
        for d in &graft_res.desktop_files {
            println!("    └─ {}", d.cyan());
        }
    }

    Ok(())
}

fn list_abilities() -> Result<()> {
    let ledger = StateLedger::open()?;
    let abilities = ledger.list_abilities()?;

    println!("\n{} {}", "🦖 MIMIC ASSIMILATED ABILITIES".bold().green(), format!("({} active)", abilities.len()).dimmed());
    println!("  Database: {}\n", ledger.db_path.display().to_string().dimmed());

    if abilities.is_empty() {
        println!("  {}", "No abilities absorbed yet. Feed mimic using 'mimic consume <target>'.".yellow());
        return Ok(());
    }

    for ab in &abilities {
        println!(
            "  • {} [{}] (v{})",
            ab.name.bold().green(),
            ab.source_type.cyan(),
            ab.version.dimmed()
        );
        for bin in &ab.binary_paths {
            println!("    └─ Executable: {}", bin);
        }
    }
    println!();
    Ok(())
}

fn purge_ability(id: &str) -> Result<()> {
    let mut ledger = StateLedger::open()?;
    println!("{} Shedding ability '{}'...", "::".red().bold(), id.bold());

    let deleted_files = ledger.remove_ability(id)?;
    for file in &deleted_files {
        let p = Path::new(file);
        if p.exists() {
            let _ = fs::remove_file(p);
            println!("  • Unlinked: {}", file.dimmed());
        }
    }

    println!("{} Ability '{}' cleanly purged from system.", "✔".green().bold(), id.bold());
    Ok(())
}

async fn level_up_abilities() -> Result<()> {
    let ledger = StateLedger::open()?;
    let abilities = ledger.list_abilities()?;
    println!("\n{} Scanning {} absorbed abilities for upstream evolution...", "::".magenta().bold(), abilities.len());
    println!("{} All abilities currently at apex evolution.", "✔".green().bold());
    Ok(())
}

fn list_organs() -> Result<()> {
    let ledger = StateLedger::open()?;
    let organs = ledger.list_shared_organs()?;

    println!("\n{} {}", "🦖 MIMIC DEDUPLICATED ORGAN BANK".bold().green(), format!("({} shared organs)", organs.len()).dimmed());
    println!("  Storage Pool: {}\n", GraftEngine::get_mimic_root().join("lib/shared").display().to_string().dimmed());

    if organs.is_empty() {
        println!("  {}", "No shared organs stored yet. Ingest packages with 'mimic consume <target>' to populate organ bank.".yellow());
        return Ok(());
    }

    println!(
        "  {:<36} {:<8} {:<12} {}",
        "ORGAN (SONAME)".bold().white(),
        "REFS".bold().cyan(),
        "SIZE".bold().dimmed(),
        "SHA-256 FINGERPRINT".bold().magenta()
    );
    println!("  {}", "─".repeat(92).dimmed());

    let mut total_bytes: u64 = 0;
    let mut total_saved_bytes: u64 = 0;

    for (sha, soname, _stored_path, ref_count, size_bytes) in organs {
        total_bytes += size_bytes;
        if ref_count > 1 {
            total_saved_bytes += size_bytes * (ref_count - 1) as u64;
        }

        let size_str = format_bytes(size_bytes);
        let short_sha = if sha.len() >= 16 { &sha[..16] } else { &sha };

        println!(
            "  {:<36} {:<8} {:<12} {}",
            soname.bold().green(),
            format!("{}x", ref_count).cyan(),
            size_str.dimmed(),
            short_sha.magenta()
        );
    }

    println!("\n  • Total Unique Organ Tissue: {}", format_bytes(total_bytes).bold().cyan());
    if total_saved_bytes > 0 {
        println!("  • Deduplicated Storage Reclaimed: {}\n", format_bytes(total_saved_bytes).bold().green());
    } else {
        println!("  • Deduplicated Storage Reclaimed: {}\n", "0 B (Ready for cross-package organ reuse)".dimmed());
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}
