use clap::Parser;
use josh_proxy::serve::{CapabilitiesDirection, encode_info_refs_response, git_list_capabilities};

#[derive(Debug, Clone, Copy)]
enum GitLib {
    Gix,
    Git2,
}

impl std::str::FromStr for GitLib {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "gix" => Ok(GitLib::Gix),
            "git2" => Ok(GitLib::Git2),
            _ => Err(format!("Invalid gitlib: {}, expected 'gix' or 'git2'", s)),
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    repo_dir: std::path::PathBuf,

    /// Git library to use for listing refs
    #[arg(long, default_value = "gix")]
    gitlib: GitLib,

    /// Ignore --bench flag passed by cargo bench
    #[arg(long)]
    bench: bool,
}

fn list_refs_gix(repo_path: &std::path::Path) -> anyhow::Result<Vec<(String, String)>> {
    let repo = gix::open(repo_path)?;

    let mut refs = Vec::new();

    if let Ok(head) = repo.head() {
        if let Some(id) = head.id() {
            refs.push((id.to_string(), "HEAD".to_string()));
        }
    }

    for reference in repo.references()?.all()? {
        let reference = reference.map_err(|e| anyhow::anyhow!("{}", e))?;
        let name = reference.name().as_bstr().to_string();

        if let Some(id) = reference.try_id() {
            refs.push((id.to_string(), name));
        }
    }

    Ok(refs)
}

fn list_refs_git2(repo_path: &std::path::Path) -> anyhow::Result<Vec<(String, String)>> {
    let repo = git2::Repository::open(repo_path)?;

    let mut refs = Vec::new();

    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            refs.push((oid.to_string(), "HEAD".to_string()));
        }
    }

    for reference in repo.references()? {
        let reference = reference?;

        if let Some(name) = reference.name() {
            if let Some(oid) = reference.target() {
                refs.push((oid.to_string(), name.to_string()));
            }
        }
    }

    Ok(refs)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let capabilities = git_list_capabilities(&args.repo_dir, CapabilitiesDirection::ReceivePack)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let start = std::time::Instant::now();
    let refs = match args.gitlib {
        GitLib::Gix => list_refs_gix(&args.repo_dir)?,
        GitLib::Git2 => list_refs_git2(&args.repo_dir)?,
    };
    let duration = start.elapsed();
    eprintln!("Time (list): {:?}", duration);
    eprintln!("N refs: {}", refs.len());

    let start = std::time::Instant::now();
    let output = encode_info_refs_response(
        &refs,
        &capabilities,
        CapabilitiesDirection::ReceivePack,
        None,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    let duration = start.elapsed();
    eprintln!("Time (encode): {:?}", duration);

    std::hint::black_box(output);

    Ok(())
}
