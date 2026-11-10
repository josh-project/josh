use super::*;
pub(super) fn set_store_head(repository: &gix::Repository) -> anyhow::Result<()> {
    repository.edit_reference(gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: Default::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: gix::refs::Target::Symbolic(SUPER_REF.try_into()?),
        },
        name: "HEAD".try_into()?,
        deref: false,
    })?;
    Ok(())
}
pub(super) fn parse_head_branch(output: &str) -> Option<&str> {
    output
        .lines()
        .next()?
        .split('\t')
        .next()?
        .strip_prefix("ref: refs/heads/")
}

pub(super) fn check_remote_name(name: &str) -> anyhow::Result<()> {
    if name.contains('/') {
        bail!("Remote names must not contain '/'");
    }
    gix::validate::reference::name_partial(name.as_bytes().into())
        .with_context(|| format!("Invalid remote name '{name}'"))?;
    Ok(())
}

pub(super) fn git_success<const N: usize>(cwd: &Path, args: [&str; N]) -> anyhow::Result<bool> {
    Ok(Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

pub(super) fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> anyhow::Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("Failed to run git")
}

pub(super) fn ensure_success(output: Output, context: &str) -> anyhow::Result<String> {
    if !output.status.success() {
        bail!(
            "{}: {}",
            context,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("Git output was not UTF-8")
}

pub(super) fn run_git<I, S>(cwd: &Path, args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("Failed to run git")?;
    ensure_success(output, "Git command failed")?;
    Ok(())
}
