use std::path::PathBuf;
use std::time::Duration;
use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BrainRequest {
    Ping,
    Why {
        package: String,
    },
    Diagnose {
        package: String,
        error_log: String,
        compiler: Option<String>,
        flags: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BrainResponse {
    Pong {
        version: String,
        status: String,
    },
    Why {
        package: String,
        summary: String,
        role: String,
        key_insights: Vec<String>,
        alternatives: Vec<String>,
        archwiki_topic: Option<String>,
    },
    Diagnose {
        package: String,
        root_cause: String,
        explanation: String,
        suggested_fixes: Vec<String>,
        suggested_flags: Option<String>,
    },
    Error {
        message: String,
    },
}

pub struct AdvisorClient {
    socket_path: PathBuf,
}

impl AdvisorClient {
    pub fn new() -> Self {
        let default_sock = PathBuf::from("/run/mimic/brain.sock");
        let socket_path = if default_sock.exists() {
            default_sock
        } else if let Ok(home) = std::env::var("HOME") {
            let user_sock = PathBuf::from(home).join(".cache/mimic/brain.sock");
            if user_sock.exists() {
                user_sock
            } else {
                default_sock
            }
        } else {
            default_sock
        };

        Self { socket_path }
    }

    /// Query the AI advisor about why a package is needed or how it works
    pub async fn query_why(&self, package: &str) -> Result<Option<BrainResponse>> {
        let req = BrainRequest::Why {
            package: package.to_string(),
        };
        self.send_request(&req).await
    }

    /// Send a compilation or transaction failure log for diagnostic triage
    pub async fn triage_failure(&self, package: &str, error_log: &str) -> Result<Option<BrainResponse>> {
        let req = BrainRequest::Diagnose {
            package: package.to_string(),
            error_log: error_log.to_string(),
            compiler: Some("gcc/makepkg".to_string()),
            flags: Some("-march=native -O3".to_string()),
        };
        self.send_request(&req).await
    }

    async fn send_request(&self, req: &BrainRequest) -> Result<Option<BrainResponse>> {
        if !self.socket_path.exists() {
            return Ok(None);
        }

        let stream = match tokio::time::timeout(Duration::from_millis(300), UnixStream::connect(&self.socket_path)).await {
            Ok(Ok(s)) => s,
            _ => return Ok(None),
        };

        let (reader, mut writer) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);

        let mut req_json = serde_json::to_string(req)?;
        req_json.push('\n');

        writer.write_all(req_json.as_bytes()).await
            .context("Failed to write request to mimic-brain IPC socket")?;

        let mut line = String::new();
        let read_future = buf_reader.read_line(&mut line);
        match tokio::time::timeout(Duration::from_secs(5), read_future).await {
            Ok(Ok(n)) if n > 0 => {
                let resp: BrainResponse = serde_json::from_str(line.trim())
                    .context("Failed to parse response from mimic-brain")?;
                Ok(Some(resp))
            }
            _ => Ok(None),
        }
    }
}

pub fn print_advisor_response(resp: &BrainResponse) {
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
            println!("\n{} AI Diagnostic Triage for '{}':", "🧠".magenta().bold(), package.bold());
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
            println!("{} Advisor Error: {}", "✖".red(), message);
        }
    }
}
