use anyhow::Context;
use std::process::{Command, Stdio};

use super::{SIDECAR_NETWORK, align_artifact, host_identity};
use crate::{Mount, NetworkPolicy, RunArgs, RunOutput};

pub(super) fn run(args: RunArgs) -> anyhow::Result<RunOutput> {
    // Defensively fix ownership of read-only mounts — pre-existing artifacts such
    // as dependency outputs, which may have been restored from a remote cache with
    // root ownership — so the step can read them.
    for mount in &args.mounts {
        if mount.read_only {
            align_artifact(&mount.artifact)?;
        }
    }

    let mut cmd = Command::new("podman");
    cmd.args(["run", "--rm"]);

    // Run as the invoking user so files land with the right ownership.
    let identity = host_identity();
    cmd.args(["--user", &identity]);

    // When sidecars are present, attach the step to the internal sidecar network
    // so it can reach them; otherwise honor the requested policy.
    let network = if !args.sidecars.is_empty() {
        SIDECAR_NETWORK.to_string()
    } else {
        match args.network {
            NetworkPolicy::Host => "host".to_string(),
            NetworkPolicy::None => "none".to_string(),
        }
    };
    cmd.args(["--network", &network]);

    if let Some(workdir) = &args.working_dir {
        cmd.args(["--workdir", workdir]);
    }

    for mount in &args.mounts {
        let spec = mount_spec(mount);
        cmd.args(["--volume", &spec]);
    }

    for (key, val) in &args.env_vars {
        cmd.args(["-e", &format!("{key}={val}")]);
    }

    // Treat the argv's first element as the executable (overriding any image
    // entrypoint) and the rest as its arguments.
    match args.command.as_slice() {
        [] => {
            cmd.arg(&args.env);
        }
        [exe, rest @ ..] => {
            cmd.args(["--entrypoint", exe]);
            cmd.arg(&args.env);
            cmd.args(rest);
        }
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to run podman container")?;

    let child_stdout = child.stdout.take().expect("stdout piped");
    let child_stderr = child.stderr.take().expect("stderr piped");

    let stdout_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut reader = child_stdout;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::Write::write_all(&mut std::io::stdout(), &chunk[..n]);
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
        buf
    });

    let stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut reader = child_stderr;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::Write::write_all(&mut std::io::stderr(), &chunk[..n]);
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
        buf
    });

    let status = child
        .wait()
        .context("failed to wait for podman container")?;
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Ok(RunOutput {
        exit_code: status.code().unwrap_or(1),
        stdout,
        stderr,
    })
}

fn mount_spec(mount: &Mount) -> String {
    if mount.read_only {
        format!("{}:{}:ro", mount.artifact, mount.path)
    } else {
        format!("{}:{}", mount.artifact, mount.path)
    }
}
