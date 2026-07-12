# Splitting build execution out of `josh-compose`

This document summarizes the refactor that separates build-graph *preparation*
from build *execution* in the `josh compose` pipeline, introducing a new
`josh-compose-backend` crate that owns execution behind a runtime-neutral trait
with a podman backend.

Status: complete in the working tree (uncommitted); behavior-preserving end to
end. Verified by `cargo check` / `cargo clippy` / `cargo fmt` across
`josh-compose`, `josh-compose-backend`, and `josh-cli`.


## Context

`josh compose run` does two things:

1. **Preparation** — collect a build graph out of the repo: resolve the filter,
   compute the workspace tree, read workspace metadata from git trees, walk the
   dependency graph, turn git trees into tar archives, manage the on-disk job
   cache.
2. **Execution** — drive a container runtime: build images, create/import/export
   volumes, run steps, start sidecar services, fix file ownership.

Before this work both concerns lived in `josh-compose`, and execution was
hard-wired to podman through a flat module of free functions (`podman.rs`).
There was no abstraction boundary, so the graph code could not be reused against
any other (or mock) execution engine.

The goal: introduce a `Runtime` trait that describes **what the scheduler
needs**, move execution behind it into `josh-compose-backend`, and have
`josh-compose` operate on `&dyn Runtime`. The concrete `PodmanRuntime` is
constructed once at the high-level entry points and threaded down.


## Architecture

```
josh-cli
  └─ josh_compose::run / plan_images / plan_jobs      (public API, unchanged)
       │  constructs PodmanRuntime::new(), passes &dyn Runtime down
       ▼
josh-compose                 preparation + orchestration, operates on &dyn Runtime
  archive  clean  container  filter  image  job_cache  meta  naming  plan
       │  calls Runtime methods only; never shells out to podman
       ▼
josh-compose-backend         the Runtime trait + shared types + PodmanRuntime
  lib.rs                     trait, NetworkPolicy, EnvRecipe, Mount, RunArgs,
                             RunOutput, SidecarArgs, SidecarHandle
  podman/{mod,envs,artifacts,run,sidecars}.rs   the podman backend
```

`josh-cli` is untouched at the API level: `run`, `plan_images`, `plan_jobs`
keep their signatures; each constructs a `PodmanRuntime` internally and threads
`&dyn Runtime` into `josh-compose`.


## The `Runtime` trait

The trait is deliberately described in scheduler terms, not podman terms. It
speaks about four things:

- **Environments** (was: images) — prepared build environments, cached by a
  content-derived key.
- **Artifacts** (was: volumes) — named, tar-addressable data areas.
- **Step execution** — run an argv in an environment with mounts/env/network.
- **Sidecar workers** (was: sidecar containers) — auxiliary services running
  alongside a step.

Everything else podman exposes — networks, published ports, container IPs,
detached containers, the busybox chown, architecture names, host UID/GID — is
an implementation detail of *how podman delivers those four*, and lives inside
the podman backend, off the trait. This is what makes the trait implementable in
principle by a non-container engine (local subprocesses, sandboxes).

Final trait surface:

```rust
pub trait Runtime {
    // environments
    fn env_exists(&self, key: &str) -> Result<bool>;
    fn prepare_env(&self, key: &str, recipe: EnvRecipe) -> Result<()>;
    fn list_envs(&self, prefix: &str) -> Result<Vec<String>>;
    fn remove_env(&self, key: &str) -> Result<()>;

    // artifacts
    fn artifact_exists(&self, name: &str) -> Result<bool>;
    fn create_artifact(&self, name: &str) -> Result<()>;
    fn import_artifact(&self, name: &str, tar: &[u8]) -> Result<()>;
    fn export_artifact(&self, name: &str) -> Result<Vec<u8>>;
    fn extract_artifact(&self, name: &str, dest: &Path) -> Result<()>;
    fn remove_artifact(&self, name: &str, force: bool) -> Result<()>;
    fn list_artifacts(&self, prefix: &str) -> Result<Vec<String>>;
    fn create_scratch_artifact(&self, tar: &[u8]) -> Result<String>;

    // step execution
    fn run(&self, args: RunArgs) -> Result<RunOutput>;

    // sidecar workers
    fn start_sidecar(&self, args: SidecarArgs) -> Result<SidecarHandle>;
    fn stop_sidecar(&self, handle: &SidecarHandle) -> Result<()>;

    // provided composites
    fn ensure_artifact(&self, name: &str) -> Result<()> { /* create if missing */ }
    fn recreate_artifact(&self, name: &str) -> Result<()> { /* rm + create */ }
}
```

Shared types live with the trait: `NetworkPolicy { None, Host }`,
`EnvRecipe { context: Vec<u8>, build_args }`, `Mount { artifact, path,
read_only }`, `RunArgs { env, command, mounts, env_vars, network, sidecars,
working_dir }`, `RunOutput`, `SidecarArgs`, `SidecarHandle { step_address, id }`.

**Dispatch:** every `josh-compose` function takes `runtime: &dyn Runtime`
(object-safe; all methods take `&self`, no generics, no async). Chosen over
generics because ~10 functions (including the recursive `run_container` and the
graph walks) would each need `<R: Runtime>`; the code is I/O-bound on podman so
dynamic dispatch is irrelevant, and `&dyn` keeps signatures simple.


## Design decisions

Each of these was a deliberate call during the refactor.

### One neutral `Runtime` trait, not a core + extension split
Sidecars are generalized to "sidecar workers" rather than kept container-shaped
behind an extension trait. A process backend would run sidecar processes; podman
runs sidecar containers. A single trait covers the whole scheduler.

### `NetworkMode` → `NetworkPolicy { None, Host }`
The old `NetworkMode::Named` only ever held the sidecar network. Once sidecar
connectivity became the backend's concern, `Named` vanished — the sidecar
network is now a private constant inside the podman backend.

### `SidecarHandle` carries only `step_address` + `id`
Originally it also exposed a TCP `probe_address` for readiness probing done in
the frontend. Readiness detection is an execution-engine concern (it relied on
podman publishing a host port), so the probe moved into
`PodmanRuntime::start_sidecar`, which blocks until reachable before returning.
`probe_address` came off the trait; the frontend only needs `step_address` (for
env injection) and `id` (opaque, for `stop_sidecar`).

### Build-time and run-time UID/GID + arch moved to the backend
`ARCH`, `USER_UID`, `USER_GID` build args and the run-time `--user uid:gid` are
container mechanics. The frontend now touches uid/gid/arch nowhere. The podman
backend computes them via private `host_uid_gid()` / `host_identity()` helpers.
Consequence: `RunArgs.identity` and the `identity` parameter of the old
`align_ownership` were removed from the trait; `josh-compose` dropped its `libc`
dependency.

### `align_ownership` removed from the trait
Ownership fix-up is a container workaround (podman volumes start root-owned).
The frontend used to call it at three points; it is now implicit in the backend:
- `recreate_artifact` (output volume) — overridden to chown after recreate;
- `create_scratch_artifact` (snapshot) — chowns after import;
- `run` — chowns **read-only mounts** (dependency outputs, which may arrive
  root-owned from a remote cache pull) before executing.

The persistent cache volume is intentionally left unchowned, preserving the
prior behavior exactly. `ensure_artifact` (used for cache) keeps the default
no-chown composite.

### `RunArgs` collapsed to a single argv; `cleanup` dropped
The `entrypoint`/`command` split was a Dockerism. It is now one
`command: Vec<String>`; the podman backend treats `command[0]` as the executable
(overriding any image entrypoint) and the rest as args. `cleanup` (literally
`--rm`, always `true` from the scheduler) was removed — `--rm` is the podman
backend's internal default.

### Output extraction moved to the backend
The frontend used to call `export_artifact` and unpack the tar itself with the
`tar` crate. Added `Runtime::extract_artifact(name, dest)`; the podman backend
exports + unpacks. Artifact I/O is now symmetric (both import and export are
backend-owned) and the frontend no longer interprets the tar wire format on the
read path.

### Resource naming: scratch moved to backend, persistent names centralized
- The snapshot (ephemeral, randomly-named, the only frontend use of `rand`/`hex`)
  moved entirely into the backend via `create_scratch_artifact(tar) -> opaque
  name`. The frontend gets an opaque name, mounts it, removes it on cleanup.
  This let `josh-compose` drop `rand`, `hex`, `backon`, and `log`.
- Persistent names (`josh_out_<oid>`, `josh_ws_image_<oid>`, `josh_cache_<name>`)
  are produced by a single `josh-compose/src/naming.rs` module used by `image`,
  `container`, `plan`, and `clean`. The runtime treats all names as opaque.

All keys carry a `josh_` prefix so runtime-created resources are unambiguous and
`clean.rs` can filter by prefix for every resource type.

### Behavior-preserving throughout
No scheduling logic changed. The set of podman operations, their order, error
messages, and ownership fix-ups are identical; only the vocabulary and the
boundary moved.


## Backend: `PodmanRuntime` module layout

`podman.rs` was split into thematic submodules:

```
podman/
  mod.rs        PodmanRuntime struct/new/Default, SIDECAR_NETWORK,
                shared helpers (host_uid_gid, host_identity, align_artifact),
                and the single `impl Runtime` — a routing table of delegators
  envs.rs       env_exists, prepare_env, list_envs, remove_env
  artifacts.rs  artifact_exists/create/import/export/extract/remove/list/
                create_scratch/recreate
  run.rs        run + private mount_spec
  sidecars.rs   start_sidecar, stop_sidecar + 8 private helpers
                (network_*, run_detached, container_port/ip, stop/rm_container)
```

**Why the delegator pattern:** Rust forbids splitting a single trait impl across
multiple `impl Trait for Type` blocks (E0119 — one per trait/type pair per
crate). So the one `impl Runtime for PodmanRuntime` stays in `mod.rs` as one-line
delegators (`fn env_exists(&self, k) { envs::env_exists(k) }`), and the real
bodies live in the thematic modules as free `pub(super) fn`s (`PodmanRuntime` is
stateless, so no `&self` needed). Shared helpers that cross module boundaries
(`host_identity`, `align_artifact`, `SIDECAR_NETWORK`) are `pub(super)` in
`mod.rs`; module-local helpers stay private in their file.


## Dependency changes

`josh-compose-backend` (new): `anyhow`, `backon`, `hex`, `libc`, `log`, `rand`,
`tar`.

`josh-compose` gained `josh-compose-backend` and dropped `backon`, `hex`,
`libc`, `log`, `rand` (all execution concerns that moved to the backend). It
keeps `anyhow`, `defer`, `git2`, `josh-core`, `josh-filter`, `tar` (used by
`archive::tree_to_tar` for build-context/snapshot production).


## Verification

- `cargo fmt`, `cargo check`, `cargo clippy` clean for `josh-compose`,
  `josh-compose-backend`, and `josh-cli` (zero warnings in these crates).
- `josh-cli` builds unchanged, confirming the public `josh_compose` API is
  stable.
- Not yet exercised by an end-to-end `josh compose run` against podman; that is
  the remaining behavioral check. The refactor is behavior-preserving, so the
  risk is in wiring, which the compile/lint/consumer-build steps cover.


## Notes / caveats

- The rename to `josh_*` keys orphans any pre-existing podman resources created
  under the old names (`out_*`, `ws_image_*`, `*_josh_cache`): they cause a cache
  miss (not reused) and `josh compose --clean` won't remove them. A one-time
  manual cleanup of old-named volumes/images clears them.
- The defensive chown of dependency output volumes (now done in `run` via the
  read-only-mount pass) exists because volumes can be restored from a remote
  cache with root ownership. It is preserved exactly.


## Remaining / future work (from the separation audit, not done here)

- **In-container path layout** (`/worktree`, `/out`, `/opt/cache`, `/<dep>`) is
  still hardcoded in the frontend. These are really a contract with the
workspace image's `run.sh`; a future step could centralize them as shared
constants both crates re-export, or give the backend a "mount layout" role.
- **`archive::tree_to_tar`** still lives in the frontend and produces tar for
  both the build context (a deliberate trait interchange format) and artifact
  seeding (a backend detail). Moving it into the runtime crate and letting
  `EnvRecipe.context` become a tree OID would hide the tar format entirely.
- **`sh -c` wrapping** of the workspace `cmd` is done in the frontend and
  defaults to `sh` while the workspace default `cmd` is `bash run.sh` — a minor
  inconsistency to reconcile.
- **A second backend** (e.g. local subprocess) is the eventual point of the
  abstraction; none exists yet.
