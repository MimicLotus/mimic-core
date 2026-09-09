use serde::{Deserialize, Serialize};

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
