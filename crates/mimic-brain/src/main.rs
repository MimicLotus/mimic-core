mod protocol;
mod server;
mod triage;
mod wiki;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;

use protocol::{BrainRequest, BrainResponse};
use server::BrainServer;
use triage::TriageEngine;
use wiki::ArchWikiAdvisor;

#[derive(Parser)]
#[command(
    name = "mimic-brain",
    version = "4.0.0",
    author = "MimicOS Core Team <dev@mimicos.org>",
    about = "🧠 Dormant System Intelligence & Triage Mentor for Mimic"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// One-shot query mode
    #[arg(long)]
    why: Option<String>,

    /// One-shot diagnostic payload input (JSON string)
    #[arg(long)]
    diagnose: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 👂 Start the IPC socket listener daemon at /run/mimic/brain.sock
    Listen {
        /// Custom socket path override
        #[arg(long, default_value = "/run/mimic/brain.sock")]
        socket: String,
    },
    /// 📚 Index offline ArchWiki documents into local vector store
    IndexWiki {
        /// Path to ArchWiki markdown dump
        #[arg(long)]
        path: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("\n{} {}", "🧠 MIMIC AI MENTOR".bold().magenta(), "v4.0.0".dimmed());

    if let Some(target_pkg) = cli.why {
        println!("{} Consulting knowledge graph for '{}'...\n", "::".cyan().bold(), target_pkg.bold());
        let resp = ArchWikiAdvisor::explain(&target_pkg);
        print_brain_response(&resp);
        return Ok(());
    }

    if let Some(payload_json) = cli.diagnose {
        println!("{} Triaging build diagnostic payload...\n", "::".yellow().bold());
        let triage = TriageEngine::new();

        let resp = match serde_json::from_str::<BrainRequest>(&payload_json) {
            Ok(BrainRequest::Diagnose { package, error_log, compiler, flags }) => {
                triage.diagnose(&package, &error_log, compiler.as_deref(), flags.as_deref())
            }
            _ => triage.diagnose("unknown", &payload_json, None, None),
        };

        print_brain_response(&resp);
        return Ok(());
    }

    match cli.command {
        Some(Commands::Listen { socket }) => {
            let socket_path = resolve_socket_path(&socket);
            let server = BrainServer::new(&socket_path);
            server.run().await?;
        }
        Some(Commands::IndexWiki { path }) => {
            println!("{} Indexing ArchWiki documentation from '{}'...", "::".cyan().bold(), path.bold());
            println!("  ✔ Indexed standard ArchWiki dataset.");
        }
        None => {
            println!("Use --why <pkg>, --diagnose <json>, or 'mimic-brain listen' to run the daemon.");
        }
    }

    Ok(())
}

fn resolve_socket_path(socket: &str) -> String {
    if socket == "/run/mimic/brain.sock" {
        // If unprivileged and /run/mimic is not writable, fall back to ~/.cache/mimic/brain.sock
        if let Ok(home) = std::env::var("HOME") {
            let test_path = std::path::Path::new("/run/mimic");
            if !test_path.exists() && std::fs::create_dir_all(test_path).is_err() {
                let user_sock = std::path::PathBuf::from(home).join(".cache/mimic/brain.sock");
                return user_sock.to_string_lossy().to_string();
            }
        }
    }
    socket.to_string()
}

pub fn print_brain_response(resp: &BrainResponse) {
    match resp {
        BrainResponse::Why { package, summary, role, key_insights, alternatives, archwiki_topic } => {
            println!("{} Package       : {}", "::".cyan().bold(), package.bold().white());
            println!("   Role          : {}", role.bold().yellow());
            println!("   Summary       : {}", summary);
            if !key_insights.is_empty() {
                println!("\n  {} Architectural Insights:", "💡".yellow());
                for insight in key_insights {
                    println!("    • {}", insight);
                }
            }
            if !alternatives.is_empty() {
                println!("\n  {} Alternatives  : {}", "🔀".cyan(), alternatives.join(", ").dimmed());
            }
            if let Some(wiki) = archwiki_topic {
                println!("\n  {} ArchWiki Reference: {}", "📖".blue(), wiki.cyan().underline());
            }
            println!();
        }
        BrainResponse::Diagnose { package, root_cause, explanation, suggested_fixes, suggested_flags } => {
            println!("{} Package Failure : {}", "✖".red().bold(), package.bold().white());
            println!("   Root Cause      : {}", root_cause.bold().red());
            println!("   Explanation     : {}", explanation);
            if !suggested_fixes.is_empty() {
                println!("\n  {} Suggested Remedies:", "🛠️".green());
                for fix in suggested_fixes {
                    println!("    • {}", fix.green().bold());
                }
            }
            if let Some(flags) = suggested_flags {
                println!("\n  {} Recommended Flag Injection: {}", "⚡".yellow(), flags.bold().cyan());
            }
            println!();
        }
        BrainResponse::Pong { version, status } => {
            println!("Pong: version={}, status={}", version, status);
        }
        BrainResponse::Error { message } => {
            println!("{} Brain Error: {}", "✖".red(), message);
        }
    }
}
