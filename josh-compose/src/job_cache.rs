use std::path::PathBuf;

fn success_dir() -> PathBuf {
    PathBuf::from(".josh/success")
}

fn failed_dir() -> PathBuf {
    PathBuf::from(".josh/failed")
}

/// Returns true if there is a cached successful run for the given job hash.
pub fn is_cached_success(hash: &str) -> bool {
    success_dir().join(hash).exists()
}

/// Persist the result of a job run under `.josh/success/<hash>` or `.josh/failed/<hash>`.
/// The file contains the captured stdout and stderr of the container.
pub fn write_result(hash: &str, success: bool, stdout: &[u8], stderr: &[u8]) {
    let dir = if success { success_dir() } else { failed_dir() };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "[josh] warning: could not create cache dir {}: {e}",
            dir.display()
        );
        return;
    }
    let path = dir.join(hash);
    let mut content = Vec::new();
    content.extend_from_slice(b"=== STDOUT ===\n");
    content.extend_from_slice(stdout);
    if !stdout.ends_with(b"\n") && !stdout.is_empty() {
        content.push(b'\n');
    }
    content.extend_from_slice(b"=== STDERR ===\n");
    content.extend_from_slice(stderr);
    if !stderr.ends_with(b"\n") && !stderr.is_empty() {
        content.push(b'\n');
    }
    if let Err(e) = std::fs::write(&path, &content) {
        eprintln!(
            "[josh] warning: could not write cache file {}: {e}",
            path.display()
        );
    }
}

/// Remove the success marker for a cached run.
pub fn remove_success(hash: &str) -> anyhow::Result<()> {
    let path = success_dir().join(hash);
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path)
        .map_err(|e| anyhow::anyhow!("failed to remove success marker {}: {e}", path.display()))
}

/// Remove the `.josh/success` and `.josh/failed` cache directories.
pub fn clean() {
    for dir in [success_dir(), failed_dir()] {
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                eprintln!("[clean] warning: could not remove {}: {e}", dir.display());
            } else {
                eprintln!("[clean] removed {}", dir.display());
            }
        }
    }
}
