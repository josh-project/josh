use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use git2::Oid;
use serde::Serialize;

#[derive(Serialize)]
struct TraceRequest {
    session: String,
    commit: String,
    label: String,
}

enum TraceState {
    Started { session_name: String },
    NotNeeded,
}

fn state() -> &'static TraceState {
    static STATE: OnceLock<TraceState> = OnceLock::new();
    STATE.get_or_init(|| {
        if detect_test_env() {
            TraceState::Started {
                session_name: generate_session_name(),
            }
        } else {
            TraceState::NotNeeded
        }
    })
}

fn detect_test_env() -> bool {
    if std::env::var("NEXTEST").is_ok() {
        return true;
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.file_name().is_some_and(|n| n == "deps") {
                return true;
            }
        }
    }

    false
}

fn generate_session_name() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    format!("trace-{}", ts)
}

fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::blocking::Client::new)
}

pub fn trace_commit(oid: Oid, name: &str) {
    let s = state();
    if let TraceState::Started { session_name } = s {
        let request = TraceRequest {
            session: session_name.clone(),
            commit: oid.to_string(),
            label: name.to_string(),
        };

        if let Err(e) = client()
            .post("http://127.0.0.1:8765/v1/trace")
            .json(&request)
            .send()
        {
            tracing::error!("Failed to send trace: {}", e);
        }
    }
}
