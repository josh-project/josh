use anyhow::{Context, anyhow};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Stdio;

/// Normalize repo path by stripping .git suffix if present
pub fn normalize_repo_path(repo_path: &std::path::Path) -> PathBuf {
    let components = repo_path.components().collect::<Vec<_>>();

    if let Some((last, components)) = components.split_last()
        && last == &std::path::Component::Normal(".git".as_ref())
    {
        components.iter().collect()
    } else {
        repo_path.into()
    }
}

/// Spawn a git command directly to the terminal so users can see progress
/// Falls back to captured output if not in a TTY environment
pub fn spawn_git_command(
    repo_path: &std::path::Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> anyhow::Result<()> {
    log::debug!("spawn_git_command: {:?}", args);

    let cwd = normalize_repo_path(repo_path);

    let mut command = std::process::Command::new("git");
    command.current_dir(cwd).args(args);

    for (key, value) in env {
        command.env(key, value);
    }

    // Check if we're in a TTY environment
    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    let status = if is_tty {
        // In TTY: inherit stdio so users can see progress
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        command.status()?.code()
    } else {
        // Not in TTY: capture output and print stderr (for tests, CI, etc.)
        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("failed to execute git command")?;

        // Print stderr if there's any output
        if !output.stderr.is_empty() {
            let output_str = String::from_utf8_lossy(&output.stderr);
            let output_str = if let Ok(testtmp) = std::env::var("TESTTMP") {
                output_str.replace(&testtmp, "${TESTTMP}")
            } else {
                output_str.to_string()
            };

            eprintln!("{}", output_str);
        }

        output.status.code()
    };

    match status.unwrap_or(1) {
        0 => Ok(()),
        code => {
            let command = args.join(" ");
            Err(anyhow!(
                "Command exited with code {}: git {}",
                code,
                command
            ))
        }
    }
}
