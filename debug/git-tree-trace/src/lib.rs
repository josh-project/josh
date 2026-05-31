use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use git2::Oid;
use serde::Serialize;

/// Base URL of the `git-tree-viewer` HTTP server.
const VIEWER_URL: &str = "http://127.0.0.1:8765";

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
        // Trace only when a viewer is actually listening. This is probed once;
        // if no viewer answers, tracing stays disabled for the whole process —
        // notably so test runs don't push to a dead port 8765.
        if viewer_ready() {
            TraceState::Started {
                session_name: generate_session_name(),
            }
        } else {
            TraceState::NotNeeded
        }
    })
}

/// Probe the viewer's readiness endpoint. Returns `true` only if it answers
/// with success; any connection error (the usual case — no viewer running)
/// yields `false`.
fn viewer_ready() -> bool {
    client()
        .get(format!("{VIEWER_URL}/v1/ready"))
        .timeout(Duration::from_millis(500))
        .send()
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
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

pub fn trace_commit(repo: &git2::Repository, oid: Oid, name: &str) {
    // No viewer listening → nothing to push or report.
    let TraceState::Started { session_name } = state() else {
        return;
    };

    let refspec = format!("{}:refs/heads/_{}", oid, oid);
    let workdir = repo.workdir().unwrap_or_else(|| repo.path());

    let mut command = std::process::Command::new("git");
    command.arg("-C");
    command.arg(workdir);
    command.arg("push");
    command.arg(VIEWER_URL);
    command.arg(&refspec);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("git-tree-trace: failed to spawn git push: {}", e);
            return;
        }
    };

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(e) => {
            eprintln!("git-tree-trace: failed to wait for git push: {}", e);
            return;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("git-tree-trace: git push failed:\n{}", stderr);
        return;
    }

    let request = TraceRequest {
        session: session_name.clone(),
        commit: oid.to_string(),
        label: name.to_string(),
    };

    if let Err(e) = client()
        .post(format!("{VIEWER_URL}/v1/traces"))
        .json(&request)
        .send()
    {
        eprintln!("git-tree-trace: failed to send trace: {}", e);
    }
}
