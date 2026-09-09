use std::path::Path;
use anyhow::{Context, Result};
use colored::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use crate::protocol::{BrainRequest, BrainResponse};
use crate::triage::TriageEngine;
use crate::wiki::ArchWikiAdvisor;

pub struct BrainServer {
    socket_path: String,
    triage: TriageEngine,
}

impl BrainServer {
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.to_string(),
            triage: TriageEngine::new(),
        }
    }

    pub async fn run(&self) -> Result<()> {
        let path = Path::new(&self.socket_path);

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        let listener = UnixListener::bind(path)
            .with_context(|| format!("Failed to bind IPC socket at '{}'", self.socket_path))?;

        println!("{} IPC Socket ready at '{}'", "✔".green().bold(), self.socket_path.bold());
        println!("{} Listening for diagnostic triage and advisor queries...\n", "::".cyan().bold());

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let triage_engine = self.triage.clone();
                    tokio::spawn(async move {
                        let (reader, mut writer) = stream.into_split();
                        let mut buf_reader = BufReader::new(reader);
                        let mut line = String::new();

                        while let Ok(n) = buf_reader.read_line(&mut line).await {
                            if n == 0 {
                                break;
                            }

                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                line.clear();
                                continue;
                            }

                            let response = match serde_json::from_str::<BrainRequest>(trimmed) {
                                Ok(BrainRequest::Ping) => BrainResponse::Pong {
                                    version: "4.0.0".to_string(),
                                    status: "dormant_standby".to_string(),
                                },
                                Ok(BrainRequest::Why { package }) => {
                                    ArchWikiAdvisor::explain(&package)
                                }
                                Ok(BrainRequest::Diagnose { package, error_log, compiler, flags }) => {
                                    triage_engine.diagnose(&package, &error_log, compiler.as_deref(), flags.as_deref())
                                }
                                Err(err) => BrainResponse::Error {
                                    message: format!("Invalid request payload: {}", err),
                                },
                            };

                            if let Ok(mut resp_json) = serde_json::to_string(&response) {
                                resp_json.push('\n');
                                let _ = writer.write_all(resp_json.as_bytes()).await;
                            }

                            line.clear();
                        }
                    });
                }
                Err(err) => {
                    eprintln!("{} Failed to accept IPC connection: {}", "✖".red(), err);
                }
            }
        }
    }
}
