//! Execution runtime abstraction for `josh-compose`.
//!
//! The scheduler in `josh-compose` needs three things from an execution engine:
//! prepared **environments** (cached build images/toolchains), named
//! tar-addressable **artifacts** (data volumes), and **sidecar workers**
//! (auxiliary services running alongside a step). This trait describes exactly
//! those capabilities in runtime-neutral terms, so it can be backed by a
//! container engine (podman, see [`PodmanRuntime`]) or, in principle, a
//! non-container engine (local subprocesses, sandboxes).
//!
//! Container-specific details the scheduler does not care about — networks,
//! published ports, container IPs, detached containers, UID fix-ups — are not on
//! the trait; each backend implements them internally.

pub mod podman;

pub use podman::PodmanRuntime;

/// Network reachability the step itself requests (independent of sidecars).
///
/// When a step has sidecar workers, the backend connects the step to them
/// regardless of this policy (for podman that means joining the internal
/// sidecar network).
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkPolicy {
    /// No network access.
    None,
    /// Full host network access.
    Host,
}

/// Recipe for preparing an environment: a tar build context plus build arguments.
#[derive(Debug, Clone)]
pub struct EnvRecipe {
    /// Build context as a tar archive.
    pub context: Vec<u8>,
    /// Build arguments, e.g. `[("ARCH", "amd64"), ("BASE", "josh_ws_image_..")]`.
    pub build_args: Vec<(String, String)>,
}

/// An artifact mounted into a step at a path.
#[derive(Debug, Clone)]
pub struct Mount {
    /// Name of the artifact (as passed to `create_artifact` / `import_artifact`) to mount.
    pub artifact: String,
    /// Mount point inside the environment (absolute path).
    pub path: String,
    pub read_only: bool,
}

/// Arguments for running a step.
pub struct RunArgs {
    /// Environment key (e.g. an image tag) to run the step in.
    pub env: String,
    /// Full argv to execute inside the environment. The first element is the
    /// executable; the backend may use it to override the environment's default
    /// entrypoint.
    pub command: Vec<String>,
    pub mounts: Vec<Mount>,
    /// Environment variables to inject into the step's environment.
    pub env_vars: Vec<(String, String)>,
    pub network: NetworkPolicy,
    /// Sidecar workers started for this step. When non-empty, the backend wires
    /// connectivity between the step and the sidecars.
    pub sidecars: Vec<SidecarHandle>,
    /// Working directory inside the environment. When `None`, the backend uses its
    /// default (typically the environment's own `WORKDIR`).
    pub working_dir: Option<String>,
}

/// Captured result of running a step.
pub struct RunOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Arguments for starting a sidecar worker.
pub struct SidecarArgs {
    /// Logical name of the sidecar; backends may use it to tag resources.
    pub name: String,
    /// Environment key (e.g. image tag) to run the sidecar in.
    pub env: String,
    /// TCP port the sidecar binds inside its environment; the backend uses this to
    /// detect readiness.
    pub port: u16,
    /// Environment variables to inject into the sidecar worker.
    pub env_vars: Vec<(String, String)>,
}

/// Handle to a running sidecar worker.
///
/// [`Runtime::start_sidecar`] returns this only once the worker is reachable, so
/// no probe address is exposed on the trait. `step_address` is reachable from
/// inside the step and injected into its environment. `id` is opaque to the
/// scheduler and used by the backend in [`Runtime::stop_sidecar`].
#[derive(Debug, Clone)]
pub struct SidecarHandle {
    pub step_address: String,
    pub id: String,
}

/// Execution backend for `josh-compose`.
///
/// All methods take `&self` so the trait is object-safe; the scheduler holds the
/// backend as `&dyn Runtime`.
pub trait Runtime {
    // --- environments (prepared build envs, cached by content key) ---

    /// Whether the environment for `key` is already prepared.
    fn env_exists(&self, key: &str) -> anyhow::Result<bool>;
    /// Prepare the environment for `key` from `recipe` (build it). Idempotent
    /// only insofar as the caller checks [`Runtime::env_exists`] first.
    fn prepare_env(&self, key: &str, recipe: EnvRecipe) -> anyhow::Result<()>;
    /// List environment keys whose tag starts with `prefix`.
    fn list_envs(&self, prefix: &str) -> anyhow::Result<Vec<String>>;
    /// Remove a prepared environment.
    fn remove_env(&self, key: &str) -> anyhow::Result<()>;

    // --- artifacts (named, tar-addressable data) ---

    fn artifact_exists(&self, name: &str) -> anyhow::Result<bool>;
    fn create_artifact(&self, name: &str) -> anyhow::Result<()>;
    /// Seed artifact `name` with the contents of `tar`.
    fn import_artifact(&self, name: &str, tar: &[u8]) -> anyhow::Result<()>;
    /// Read artifact `name` back as a tar archive.
    fn export_artifact(&self, name: &str) -> anyhow::Result<Vec<u8>>;
    /// Unpack artifact `name` into host directory `dest`.
    fn extract_artifact(&self, name: &str, dest: &std::path::Path) -> anyhow::Result<()>;
    /// Remove an artifact; `force` requests unconditional removal.
    fn remove_artifact(&self, name: &str, force: bool) -> anyhow::Result<()>;
    /// List artifacts whose name starts with `prefix`.
    fn list_artifacts(&self, prefix: &str) -> anyhow::Result<Vec<String>>;
    /// Create a uniquely-named ephemeral artifact seeded with `tar` and return its
    /// opaque name. The caller mounts it and removes it when done. The backend
    /// fixes ownership for the invoking user as needed.
    fn create_scratch_artifact(&self, tar: &[u8]) -> anyhow::Result<String>;

    // --- step execution ---

    /// Run a step, streaming stdout/stderr to the terminal while capturing them.
    fn run(&self, args: RunArgs) -> anyhow::Result<RunOutput>;

    // --- sidecar workers ---

    /// Start a sidecar worker and block until it is reachable; return its handle.
    fn start_sidecar(&self, args: SidecarArgs) -> anyhow::Result<SidecarHandle>;
    /// Stop a previously started sidecar worker.
    fn stop_sidecar(&self, handle: &SidecarHandle) -> anyhow::Result<()>;

    // --- provided composites ---

    /// Ensure an artifact exists, creating it if missing.
    fn ensure_artifact(&self, name: &str) -> anyhow::Result<()> {
        if self.artifact_exists(name)? {
            return Ok(());
        }
        self.create_artifact(name)?;
        if !self.artifact_exists(name)? {
            anyhow::bail!("runtime artifact {name} was not created");
        }
        Ok(())
    }

    /// Remove (if present) and recreate an artifact. Container backends override
    /// this to also fix ownership for the invoking user (the default does not).
    fn recreate_artifact(&self, name: &str) -> anyhow::Result<()> {
        if self.artifact_exists(name)? {
            self.remove_artifact(name, true)?;
            if self.artifact_exists(name)? {
                anyhow::bail!("runtime artifact {name} still exists after removal");
            }
        }
        self.create_artifact(name)?;
        if !self.artifact_exists(name)? {
            anyhow::bail!("runtime artifact {name} was not created");
        }
        Ok(())
    }
}
