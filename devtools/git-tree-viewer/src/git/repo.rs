use std::path::Path;

pub fn open_repo(path: impl AsRef<Path>) -> anyhow::Result<gix::Repository> {
    Ok(gix::discover(path)?)
}

pub fn resolve_commit(repo: &gix::Repository, spec: Option<&str>) -> anyhow::Result<gix::ObjectId> {
    let spec = spec.unwrap_or("HEAD");
    Ok(repo.rev_parse_single(spec)?.object()?.peel_to_commit()?.id)
}
