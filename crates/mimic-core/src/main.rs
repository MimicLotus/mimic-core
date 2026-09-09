mod advisor_ipc;
mod alpm_engine;
mod aur;
mod builder;
mod config;
mod micro_repo;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;

use advisor_ipc::AdvisorClient;
use alpm_engine::AlpmEngine;
use aur::AurClient;
use builder::AurBuilder;
use config::PacmanConfig;
use micro_repo::MicroRepoResolver;

#[derive(Parser)]
#[command(
    name = "mimic",
    version = "4.0.0",
    author = "MimicOS Core Team <dev@mimicos.org>",
    about = "🦖 Unified Package Engine & System Intelligence for Arch Linux"
)]
struct Cli {
    /// Specify an alternate root directory for package operations (Sandbox Mode)
    #[arg(short = 'r', long = "root", global = true, value_name = "PATH")]
    root: Option<String>,

    /// Specify an alternate database location (Sandbox Mode)
    #[arg(short = 'b', long = "dbpath", global = true, value_name = "PATH")]
    dbpath: Option<String>,

    /// Specify an alternate configuration file path
    #[arg(long = "config", global = true, value_name = "PATH")]
    config: Option<String>,

    /// Disable automatic CachyOS CPU-optimized repository injection
    #[arg(long = "no-cachy", global = true)]
    no_cachy: bool,

    /// Add custom GitHub micro-repository (owner/repo)
    #[arg(long = "micro-repo", global = true, value_name = "OWNER/REPO")]
    micro_repos: Vec<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 🔄 Synchronize package databases and upgrade system (-Sy / -Syu)
    #[command(alias = "update", alias = "up", alias = "-Sy", alias = "-Syu")]
    Sync {
        /// Force refresh all package databases even if up to date (-yy)
        #[arg(short = 'y', long = "refresh")]
        refresh: bool,
    },
    /// 🔍 Search across official mirrors, micro-repos, and AUR (-Ss)
    #[command(alias = "find", alias = "-Ss")]
    Search {
        /// Target package query or keyword
        query: String,

        /// Search only the AUR
        #[arg(long = "aur")]
        aur_only: bool,
    },
    /// ℹ️ Display detailed information about a package (-Si / -Qi)
    #[command(alias = "-Si", alias = "-Qi")]
    Info {
        /// Target package name
        package: String,
    },
    /// 📦 Install packages from official mirrors, micro-repos, or AUR (-S)
    #[command(alias = "add", alias = "get", alias = "-S")]
    Install {
        /// Target packages to install
        #[arg(required = true)]
        packages: Vec<String>,

        /// Do not prompt for confirmation
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🔨 Build an AUR package inside a hermetic bwrap container
    #[command(alias = "make", alias = "compile", alias = "-B")]
    Build {
        /// Target AUR package name
        package: String,

        /// Output directory for built package artifacts
        #[arg(short = 'o', long = "output")]
        output: Option<String>,

        /// Install after successful build
        #[arg(short = 'i', long = "install")]
        install: bool,

        /// Do not prompt for confirmation
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🐙 Build and package directly from a Git source repository
    #[command(alias = "git-build", alias = "from-git")]
    Git {
        /// Git repository URL (e.g. https://github.com/user/repo)
        url: String,

        /// Optional Git branch or tag to build
        #[arg(long = "branch")]
        branch: Option<String>,

        /// Do not prompt for confirmation before installing
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,

        /// Build only without installing
        #[arg(long = "no-install")]
        no_install: bool,
    },
    /// 🗑️ Remove packages from the system (-R / -Rns)
    #[command(alias = "uninstall", alias = "rm", alias = "-R", alias = "-Rns")]
    Remove {
        /// Target packages to remove
        #[arg(required = true)]
        packages: Vec<String>,

        /// Remove all unneeded dependencies (cascade)
        #[arg(short = 'c', long = "cascade")]
        cascade: bool,

        /// Do not prompt for confirmation
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🧠 Ask the dormant AI mentor why a package is needed or how it works
    Why {
        /// Target package to inspect
        package: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let pacman_conf = PacmanConfig::load_default(cli.root.as_deref(), cli.dbpath.as_deref(), cli.no_cachy)?;
    let mut engine = AlpmEngine::new(pacman_conf)?;
    let aur_client = AurClient::new();
    let micro_resolver = MicroRepoResolver::new();

    match cli.command {
        Commands::Sync { refresh } => {
            engine.sync_databases(refresh)?;
        }
        Commands::Search { query, aur_only } => {
            println!("{} Searching package grounds for '{}'...\n", "::".cyan().bold(), query.bold());

            if !aur_only {
                let alpm_results = engine.search(&query);
                engine.print_search_results(&query, &alpm_results);
            }

            // Also search AUR
            if let Ok(aur_results) = aur_client.search(&query).await {
                if !aur_results.is_empty() {
                    if !aur_only {
                        println!();
                    }
                    for aur_pkg in aur_results.iter().take(15) {
                        let votes = aur_pkg.num_votes.unwrap_or(0);
                        let pop = aur_pkg.popularity.unwrap_or(0.0);
                        let desc = aur_pkg.description.as_deref().unwrap_or("");

                        println!(
                            "{}/{} {} {} {}",
                            "aur".bold().magenta(),
                            aur_pkg.name.bold().white(),
                            aur_pkg.version.green(),
                            format!("(+{} {:.2})", votes, pop).dimmed(),
                            "[AUR]".magenta().bold()
                        );
                        if !desc.is_empty() {
                            println!("    {}", desc.dimmed());
                        }
                    }
                }
            }
        }
        Commands::Info { package } => {
            // 1. Check localdb & syncdbs first
            if let Some(info) = engine.info(&package) {
                println!("\n{} Repository     : {}", "::".cyan().bold(), info.repo.bold().yellow());
                println!("   Name           : {}", info.name.bold().white());
                println!("   Version        : {}", info.version.green());
                println!("   Description    : {}", info.description);
                println!("   URL            : {}", info.url.cyan());
                println!("   Licenses       : {}", info.licenses.join(", "));
                println!("   Dependencies   : {}", if info.dependencies.is_empty() { "None".dimmed().to_string() } else { info.dependencies.join(", ") });
                if !info.optional_deps.is_empty() {
                    println!("   Optional Deps  : {}", info.optional_deps.join(", "));
                }
                if info.download_size > 0 {
                    println!("   Download Size  : {}", format_bytes(info.download_size as u64).cyan());
                }
                if info.installed_size > 0 {
                    println!("   Installed Size : {}", format_bytes(info.installed_size as u64).green());
                }
                println!("   Build Date     : {}", info.build_date.dimmed());
                println!("   Packager       : {}", info.packager.dimmed());
                println!("   Installed State: {}\n", if info.is_installed { "Installed".green().bold() } else { "Not installed".dimmed() });
                return Ok(());
            }

            // 2. Check Micro-Repositories if configured
            if !cli.micro_repos.is_empty() {
                if let Some(micro_pkg) = micro_resolver.find_package(&cli.micro_repos, &package).await {
                    println!("\n{} Repository     : {}", "::".cyan().bold(), format!("micro-repo ({})", micro_pkg.repo_name).bold().yellow());
                    println!("   Name           : {}", micro_pkg.name.bold().white());
                    println!("   Version        : {}", micro_pkg.version.green());
                    println!("   Architecture   : {}", micro_pkg.arch.dimmed());
                    println!("   Download URL   : {}", micro_pkg.download_url.cyan());
                    println!("   Download Size  : {}\n", format_bytes(micro_pkg.size_bytes).cyan());
                    return Ok(());
                }
            }

            // 3. Fallback to AUR RPC Info
            match aur_client.info(&package).await {
                Ok(Some(aur_pkg)) => {
                    println!("\n{} Repository     : {}", "::".cyan().bold(), "aur".bold().magenta());
                    println!("   Name           : {}", aur_pkg.name.bold().white());
                    println!("   Version        : {}", aur_pkg.version.green());
                    println!("   Description    : {}", aur_pkg.description.as_deref().unwrap_or(""));
                    println!("   URL            : {}", aur_pkg.url.as_deref().unwrap_or("").cyan());
                    println!("   Licenses       : {}", aur_pkg.licenses.join(", "));
                    println!("   Maintainer     : {}", aur_pkg.maintainer.as_deref().unwrap_or("orphan").bold());
                    println!("   Votes / Pop    : +{} (Popularity: {:.2})", aur_pkg.num_votes.unwrap_or(0), aur_pkg.popularity.unwrap_or(0.0));
                    println!("   Dependencies   : {}", if aur_pkg.depends.is_empty() { "None".dimmed().to_string() } else { aur_pkg.depends.join(", ") });
                    if !aur_pkg.make_depends.is_empty() {
                        println!("   Make Depends   : {}", aur_pkg.make_depends.join(", "));
                    }
                    if !aur_pkg.opt_depends.is_empty() {
                        println!("   Optional Deps  : {}", aur_pkg.opt_depends.join(", "));
                    }
                    if let Some(ref url_path) = aur_pkg.url_path {
                        println!("   AUR Tarball    : {}", format!("https://aur.archlinux.org{}", url_path).dimmed());
                    }
                    println!();
                }
                _ => {
                    eprintln!("{} Package '{}' not found across official mirrors, micro-repos, or AUR.", "✖".red().bold(), package.bold());
                }
            }
        }
        Commands::Install { packages, noconfirm } => {
            println!("{} Planning installation transaction for {:?}...", "::".cyan().bold(), packages);
            match engine.plan_install(&packages, false) {
                Ok(plan) => {
                    engine.print_plan(&plan);
                    if noconfirm || prompt_confirm("Proceed with installation?") {
                        engine.commit_transaction()?;
                    } else {
                        println!("{} Transaction aborted by user.", "::".yellow());
                        engine.release_transaction();
                    }
                }
                Err(err) => {
                    // If a single package failed to plan, check if it's available in AUR
                    if packages.len() == 1 {
                        let pkg = &packages[0];
                        if let Ok(Some(aur_pkg)) = aur_client.info(pkg).await {
                            println!("\n{} Package '{}' is not in binary repositories, but was found in {} (v{})",
                                "::".yellow().bold(),
                                pkg.bold().white(),
                                "AUR".magenta().bold(),
                                aur_pkg.version.green()
                            );
                            if noconfirm || prompt_confirm(&format!("Build '{}' in Hermetic Bubblewrap Sandbox?", pkg)) {
                                let builder = AurBuilder::new();
                                let built_pkgs = builder.build(pkg, None).await?;
                                let pkg_paths: Vec<String> = built_pkgs.iter().map(|p| p.to_string_lossy().to_string()).collect();
                                let plan = engine.plan_install(&pkg_paths, false)?;
                                engine.print_plan(&plan);
                                if noconfirm || prompt_confirm("Proceed with installation of built package?") {
                                    engine.commit_transaction()?;
                                } else {
                                    println!("{} Transaction aborted by user.", "::".yellow());
                                    engine.release_transaction();
                                }
                                return Ok(());
                            } else {
                                println!("{} Installation aborted.", "::".yellow());
                                return Ok(());
                            }
                        }
                    }
                    return Err(err);
                }
            }
        }
        Commands::Build { package, output, install, noconfirm } => {
            let builder = AurBuilder::new();
            let out_path = output.as_ref().map(std::path::Path::new);
            let built_packages = match builder.build(&package, out_path).await {
                Ok(pkgs) => pkgs,
                Err(build_err) => {
                    let advisor = AdvisorClient::new();
                    if let Ok(Some(diag_resp)) = advisor.triage_failure(&package, &build_err.to_string()).await {
                        advisor_ipc::print_advisor_response(&diag_resp);
                    }
                    return Err(build_err);
                }
            };

            if install {
                println!("\n{} Installing generated package artifacts...", "::".cyan().bold());
                let pkg_paths: Vec<String> = built_packages.iter().map(|p| p.to_string_lossy().to_string()).collect();
                let plan = engine.plan_install(&pkg_paths, false)?;
                engine.print_plan(&plan);

                if noconfirm || prompt_confirm("Proceed with installation of built package?") {
                    engine.commit_transaction()?;
                } else {
                    println!("{} Transaction aborted by user.", "::".yellow());
                    engine.release_transaction();
                }
            }
        }
        Commands::Git { url, branch, noconfirm, no_install } => {
            let git_runner = builder::GitSourceRunner::new();
            let pkg_artifact = match git_runner.build_from_git(&url, branch.as_deref()) {
                Ok(path) => path,
                Err(err) => {
                    let advisor = AdvisorClient::new();
                    if let Ok(Some(diag_resp)) = advisor.triage_failure(&url, &err.to_string()).await {
                        advisor_ipc::print_advisor_response(&diag_resp);
                    }
                    return Err(err);
                }
            };

            if !no_install {
                println!("\n{} Planning installation for generated Git package...", "::".cyan().bold());
                let pkg_path_str = pkg_artifact.to_string_lossy().to_string();
                let plan = engine.plan_install(&[pkg_path_str], false)?;
                engine.print_plan(&plan);

                if noconfirm || prompt_confirm("Proceed with installation of Git package?") {
                    engine.commit_transaction()?;
                } else {
                    println!("{} Transaction aborted by user.", "::".yellow());
                    engine.release_transaction();
                }
            }
        }
        Commands::Remove { packages, cascade, noconfirm } => {
            println!("{} Planning removal transaction for {:?}...", "::".red().bold(), packages);
            let plan = engine.plan_remove(&packages, cascade)?;
            engine.print_plan(&plan);

            if noconfirm || prompt_confirm("Proceed with removal?") {
                engine.commit_transaction()?;
            } else {
                println!("{} Transaction aborted by user.", "::".yellow());
                engine.release_transaction();
            }
        }
        Commands::Why { package } => {
            println!("{} Consulting dormant advisor for '{}'...\n", "::".magenta().bold(), package.bold());
            let advisor = AdvisorClient::new();
            match advisor.query_why(&package).await {
                Ok(Some(resp)) => {
                    advisor_ipc::print_advisor_response(&resp);
                }
                _ => {
                    println!("{} mimic-brain daemon is dormant (0 MB idle RAM).", "💤".blue());
                    println!("  Run 'mimic-brain --why {}' for one-shot offline explanation,", package);
                    println!("  or 'mimic-brain listen' to activate real-time socket triage.\n");
                }
            }
        }
    }

    Ok(())
}


fn prompt_confirm(prompt: &str) -> bool {
    use std::io::{stdin, stdout, Write};
    print!("{} {} [Y/n]: ", "::".yellow().bold(), prompt.bold());
    let _ = stdout().flush();

    let mut input = String::new();
    if stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim().to_lowercase();
        trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
    } else {
        false
    }
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
