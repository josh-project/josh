use libc::signal;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context};
use clap::Parser;
use gix::bstr::ByteSlice;
use gix::objs::tree::EntryKind;
use gix::objs::{Find, FindExt, Write};
use gix::status::index_worktree::iter::Summary;

/// Pretty print a git tree, tree(1)-style, including blob contents.
#[derive(Parser)]
struct Args {
    /// Commit or tree to print (defaults to HEAD)
    rev: Option<String>,
    /// Path to the repository (defaults to the current directory)
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Omit blob contents, only print the structure
    #[arg(long)]
    no_contents: bool,
}

fn main() -> ExitCode {
    // Rust ignores SIGPIPE; restore the default so pipes like `| head`
    // terminate us instead of panicking with EPIPE on every print.
    unsafe { signal(libc::SIGPIPE, libc::SIG_DFL) };
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> anyhow::Result<()> {
    let repo = gix::open(&args.repo).context("failed to open repository")?;
    let rev = args.rev.as_deref().unwrap_or("HEAD");
    let tree_oid = match rev {
        "+" => index_tree(&repo)?,
        "." => worktree_tree(&repo)?,
        _ => resolve_tree(&repo, rev)?,
    };

    println!(".");
    print_tree(&repo.objects, &tree_oid, "", args.no_contents)
}

fn resolve_tree(repo: &gix::Repository, rev: &str) -> anyhow::Result<gix::ObjectId> {
    let id = repo
        .rev_parse_single(rev)
        .with_context(|| format!("failed to resolve {rev:?}"))?
        .detach();
    let mut buf = Vec::new();
    let obj = repo
        .objects
        .find(&id, &mut buf)
        .with_context(|| format!("object {id} not found"))?;

    match obj.kind {
        gix::object::Kind::Tree => Ok(id),
        gix::object::Kind::Commit => Ok(gix::objs::CommitRef::from_bytes(obj.data, id.kind())
            .context("malformed commit")?
            .tree()),
        _ => bail!(
            "{rev} resolves to a {}, expected a commit or tree",
            obj.kind
        ),
    }
}

fn index_tree(repo: &gix::Repository) -> anyhow::Result<gix::ObjectId> {
    let index = repo.index().context("failed to open index")?;
    write_index_tree(repo, &index)
}

fn write_index_tree(
    repo: &gix::Repository,
    index: &gix::index::State,
) -> anyhow::Result<gix::ObjectId> {
    let mut editor = gix::objs::tree::Editor::new(
        gix::objs::Tree::default(),
        &repo.objects,
        repo.object_hash(),
    );
    for entry in index.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            bail!("cannot print an index with unresolved conflicts");
        }
        let mode = entry
            .mode
            .to_tree_entry_mode()
            .context("index entry has an invalid mode")?;
        editor.upsert(entry.path(index).split_str("/"), mode.kind(), entry.id)?;
    }
    editor
        .write(|tree| repo.objects.write(tree))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn worktree_tree(repo: &gix::Repository) -> anyhow::Result<gix::ObjectId> {
    let head_tree = resolve_tree(repo, "HEAD")?;
    let head_index = repo.index_from_tree(&head_tree)?;
    let mut buf = Vec::new();
    let root = repo.objects.find_tree(&head_tree, &mut buf)?.into();
    let mut editor = gix::objs::tree::Editor::new(root, &repo.objects, repo.object_hash());
    let (mut pipeline, filter_index) = repo.filter_pipeline(None)?;
    let status = repo
        .status(gix::progress::Discard)?
        .index(gix::worktree::IndexPersistedOrInMemory::InMemory(
            head_index.clone(),
        ))
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_submodules(None)
        .into_index_worktree_iter(Vec::new())?;

    for item in status {
        let item = item?;
        let Some(summary) = item.summary() else {
            continue;
        };
        let path = item.rela_path();
        match summary {
            Summary::Removed => {
                editor.remove(path.split_str("/"))?;
            }
            Summary::Conflict => bail!("cannot print a working tree with unresolved conflicts"),
            Summary::Added
            | Summary::Modified
            | Summary::TypeChange
            | Summary::IntentToAdd
            | Summary::Renamed
            | Summary::Copied => match pipeline.worktree_file_to_object(path, &filter_index)? {
                Some((id, kind, _)) => {
                    editor.upsert(path.split_str("/"), kind, id)?;
                }
                None => {
                    editor.remove(path.split_str("/"))?;
                }
            },
        }
    }

    editor
        .write(|tree| repo.objects.write(tree))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn print_tree(
    odb: &impl Find,
    oid: &gix::hash::oid,
    prefix: &str,
    no_contents: bool,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let tree = odb.find_tree(oid, &mut buf)?;
    let entries = &tree.entries;

    for (i, entry) in entries.iter().enumerate() {
        let last = i + 1 == entries.len();
        let connector = if last { "└── " } else { "├── " };
        let name = String::from_utf8_lossy(entry.filename);
        let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });

        match entry.mode.kind() {
            EntryKind::Tree => {
                println!("{prefix}{connector}{name}/");
                print_tree(odb, entry.oid, &child_prefix, no_contents)?;
            }
            EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link => {
                println!("{prefix}{connector}{name}");
                if !no_contents {
                    let mut data = Vec::new();
                    odb.find_blob(entry.oid, &mut data)?;
                    print_contents(&data, &child_prefix);
                }
            }
            EntryKind::Commit => println!("{prefix}{connector}{name} (submodule)"),
        }
    }
    Ok(())
}

fn print_contents(data: &[u8], prefix: &str) {
    // Binary or invalid UTF-8: hex dump instead of mangled bytes.
    let Ok(text) = std::str::from_utf8(data) else {
        print_hex(data, prefix);
        return;
    };
    if text.is_empty() {
        return;
    }
    for raw_line in text.split_inclusive('\n') {
        let (marker, line) = match raw_line.strip_suffix('\n') {
            Some(line) => ("┆", line.strip_suffix('\r').unwrap_or(line)),
            None => ("╵", raw_line),
        };
        if line.is_empty() {
            println!("{prefix}{marker}");
        } else {
            println!("{prefix}{marker}  {line}");
        }
    }
}

/// xxd-style dump: 16 bytes per line, two 8-byte groups, ASCII gutter
/// with non-printables rendered as `·`.
fn print_hex(data: &[u8], prefix: &str) {
    const BYTES_PER_LINE: usize = 16;
    // "ff fe ..." twice: 2*32 hex chars + separators + group gap.
    const HEX_WIDTH: usize = 48;

    println!("{prefix}┆  <binary, {} bytes>", data.len());
    for chunk in data.chunks(BYTES_PER_LINE) {
        let mut hex = String::new();
        let mut ascii = String::new();
        for (i, &b) in chunk.iter().enumerate() {
            if i > 0 {
                hex.push_str(if i % 8 == 0 { "  " } else { " " });
            }
            use std::fmt::Write as _;
            write!(hex, "{b:02x}").expect("write to String cannot fail");
            ascii.push(if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '·'
            });
        }
        println!("{prefix}┆  {hex:<HEX_WIDTH$}  {ascii}");
    }
}
