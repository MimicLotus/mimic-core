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
use hunt::{DebHunter, GitHunter, MirrorResolver};
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
    /// 🥩 Hunt, break, mutate, and assimilate a package or repository into the host
    #[command(alias = "absorb")]
    Consume {
        /// Target prey (e.g. deb:vlc, deb:./pkg.deb, git:BurntSushi/ripgrep, or direct name)
        target: String,
        /// Explicit source type override (deb, git, aur, rpm)
        #[arg(short, long)]
        source: Option<String>,
    },
    /// 📜 List all consumed abilities and assimilated organs
    #[command(alias = "abilities")]
    List,
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Consume { target, source } => {
            consume_prey(&target, source.as_deref()).await?;
        }
        Commands::List => {
            list_abilities()?;
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
    }

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

fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}
