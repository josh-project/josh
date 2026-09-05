use anyhow::Context;
use backon::{BlockingRetryable, ExponentialBuilder};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::SIDECAR_NETWORK;
use josh_compose_backend::{SidecarArgs, SidecarHandle};

/// Start a sidecar worker and block until it is reachable.
///
/// Lifecycle:
/// 1. Ensure the internal sidecar network exists (create if missing).
/// 2. Launch the sidecar image as a detached container with a random name on
///    the default bridge (so the sidecar can reach the internet during
///    startup), then attach it to the internal sidecar network.
/// 3. Extract the container's IP on the sidecar network (fail → stop + rm).
/// 4. Probe the published port with exponential backoff; if the sidecar
///    never becomes reachable, stop + rm the container and return an error.
/// 5. Return a [`SidecarHandle`] with the step-reachable address and the
///    opaque container id.
pub(super) fn start_sidecar(args: SidecarArgs) -> anyhow::Result<SidecarHandle> {
    ensure_network_internal(SIDECAR_NETWORK)?;

    let bytes: [u8; 4] = rand::random();
    let container_name = format!("josh-sidecar-{}-{}", args.name, hex::encode(bytes));

    run_detached(&args.env, &container_name, &args.env_vars, args.port)?;

    // `docker run` accepts only one `--network`, so the internal sidecar
    // network is attached after the container starts on the default bridge.
    if let Err(e) = network_connect(SIDECAR_NETWORK, &container_name) {
        stop_container(&container_name);
        rm_container_force(&container_name);
        return Err(e);
    }

    let step_address = match container_ip(&container_name, SIDECAR_NETWORK) {
        Ok(ip) => ip,
        Err(e) => {
            stop_container(&container_name);
            rm_container_force(&container_name);
            return Err(e);
        }
    };

    let probe_address =
        container_port(&container_name, args.port).context("sidecar port not published")?;

    let probe = || -> std::io::Result<()> {
        TcpStream::connect_timeout(&probe_address, Duration::from_millis(200)).map(|_| ())
    };
    let backoff = ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(25))
        .with_max_delay(Duration::from_millis(250))
        .with_max_times(50)
        .with_jitter();
    if let Err(e) = probe.retry(backoff).call() {
        stop_container(&container_name);
        rm_container_force(&container_name);
        anyhow::bail!(
            "sidecar {} not reachable at {probe_address} within timeout: {e}",
            args.name
        );
    }

    Ok(SidecarHandle {
        step_address,
        id: container_name,
    })
}

pub(super) fn stop_sidecar(handle: &SidecarHandle) -> anyhow::Result<()> {
    stop_container(&handle.id);
    rm_container_force(&handle.id);
    Ok(())
}

fn ensure_network_internal(name: &str) -> anyhow::Result<()> {
    if !network_exists(name)? {
        network_create_internal(name)?;
    }
    Ok(())
}

fn network_exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("docker")
        .args(["network", "inspect", "--format", "{{.Name}}", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run docker network inspect")?;
    Ok(status.success())
}

fn network_create_internal(name: &str) -> anyhow::Result<()> {
    let output = Command::new("docker")
        .args(["network", "create", "--internal", name])
        .output()
        .context("failed to run docker network create")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker network create {name} failed: {stderr}");
    }
    Ok(())
}

fn network_connect(network: &str, container: &str) -> anyhow::Result<()> {
    let output = Command::new("docker")
        .args(["network", "connect", network, container])
        .output()
        .context("failed to run docker network connect")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker network connect {network} {container} failed: {stderr}");
    }
    Ok(())
}

fn run_detached(
    image: &str,
    name: &str,
    env_vars: &[(String, String)],
    port: u16,
) -> anyhow::Result<()> {
    let mut cmd = Command::new("docker");
    cmd.args(["run", "--rm", "-d", "--name", name]);
    for (key, val) in env_vars {
        cmd.args(["-e", &format!("{key}={val}")]);
    }
    cmd.args(["-p", &format!("{port}")]);
    cmd.arg(image);

    let output = cmd.output().context("failed to run docker run --detach")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker run --detach {name} failed: {stderr}");
    }
    Ok(())
}

fn container_port(container: &str, port: u16) -> anyhow::Result<std::net::SocketAddr> {
    let output = Command::new("docker")
        .args(["port", container, &format!("{port}")])
        .output()
        .context("failed to run docker port")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker port {container} {port} failed: {stderr}");
    }
    let binding = String::from_utf8_lossy(&output.stdout).trim().to_string();
    binding
        .parse()
        .with_context(|| format!("docker port {container} {port}: invalid output: {binding}"))
}

fn container_ip(container: &str, network: &str) -> anyhow::Result<String> {
    // Use `index` rather than dotted access so hyphens in `network` don't break Go-template parsing.
    let format = format!("{{{{(index .NetworkSettings.Networks \"{network}\").IPAddress}}}}");
    let output = Command::new("docker")
        .args(["inspect", "--format", &format, container])
        .output()
        .context("failed to run docker inspect")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker inspect {container} failed: {stderr}");
    }
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ip.is_empty() {
        anyhow::bail!("container {container} has no IP on network {network}");
    }
    Ok(ip)
}

fn stop_container(name: &str) {
    let output = Command::new("docker").args(["stop", name]).output();
    match output {
        Ok(o) if !o.status.success() => {
            log::warn!(
                "docker stop {name} failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => log::warn!("failed to run docker stop: {e}"),
        _ => {}
    }
}

fn rm_container_force(name: &str) {
    let output = Command::new("docker").args(["rm", "-f", name]).output();
    match output {
        Ok(o) if !o.status.success() => {
            log::warn!(
                "docker rm -f {name} failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => log::warn!("failed to run docker rm: {e}"),
        _ => {}
    }
}
