use crate::evaluate::evaluate;
use josh_filter::spec;
use std::str::FromStr;

#[test]
fn test_simple_filter() -> anyhow::Result<()> {
    let temp_dir = std::env::temp_dir().join("josh_starlark_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = gix::init(&temp_dir)?;
    let empty_tree_oid = gix_hash::ObjectId::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;

    let script = r#"
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, empty_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_chain_filter() -> anyhow::Result<()> {
    let temp_dir = std::env::temp_dir().join("josh_starlark_test2");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = gix::init(&temp_dir)?;
    let empty_tree_oid = gix_hash::ObjectId::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;

    let script = r#"
filter = filter.subdir("src").prefix("lib")
"#;

    let filter = evaluate(script, empty_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src:prefix=lib");
    Ok(())
}

#[test]
fn test_file_filter() -> anyhow::Result<()> {
    let temp_dir = std::env::temp_dir().join("josh_starlark_test3");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = gix::init(&temp_dir)?;
    let empty_tree_oid = gix_hash::ObjectId::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;

    let script = r#"
filter = filter.file("README.md")
"#;

    let filter = evaluate(script, empty_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    // file() creates a rename from the same path to itself, which is represented as ::README.md
    assert_eq!(filter_spec, "::README.md");
    Ok(())
}

#[test]
fn test_compose() -> anyhow::Result<()> {
    let temp_dir = std::env::temp_dir().join("josh_starlark_test4");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = gix::init(&temp_dir)?;
    let empty_tree_oid = gix_hash::ObjectId::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;

    let script = r#"
f1 = filter.subdir("src")
f2 = filter.subdir("lib")
filter = compose([f1, f2])
"#;

    let filter = evaluate(script, empty_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    // compose formats as :[filter1,filter2]
    assert_eq!(filter_spec, ":[:/src,:/lib]");
    Ok(())
}

fn write_tree(
    objects: &impl gix_object::Write,
    mut entries: Vec<gix_object::tree::Entry>,
) -> anyhow::Result<gix_hash::ObjectId> {
    entries.sort();
    objects
        .write(&gix_object::Tree { entries })
        .map_err(|err| anyhow::anyhow!("write test tree: {err}"))
}

// Helper function to create a test repository with files and directories
fn create_test_repo() -> anyhow::Result<(gix::Repository, gix_hash::ObjectId)> {
    // Process id and a counter keep the directory unique across parallel tests; the
    // timestamp alone collides when two tests read the clock in the same tick.
    static DIR_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let temp_dir = std::env::temp_dir().join(format!(
        "josh_starlark_tree_test_{}_{}_{}",
        std::process::id(),
        DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = gix::init(&temp_dir)?;

    let lib_tree_oid = {
        let utils_rs_blob =
            josh_gix_ext::write_blob(&repo.objects, b"pub fn helper() {\n    // helper\n}")?;
        write_tree(
            &repo.objects,
            vec![gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Blob.into(),
                filename: "utils.rs".into(),
                oid: utils_rs_blob,
            }],
        )?
    };

    let src_tree_oid = {
        let main_rs_blob =
            josh_gix_ext::write_blob(&repo.objects, b"fn main() {\n    println!(\"Hello\");\n}")?;
        write_tree(
            &repo.objects,
            vec![
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Blob.into(),
                    filename: "main.rs".into(),
                    oid: main_rs_blob,
                },
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: "lib".into(),
                    oid: lib_tree_oid,
                },
            ],
        )?
    };

    let root_tree_oid = {
        let readme_blob =
            josh_gix_ext::write_blob(&repo.objects, b"# Project\nThis is a test project.")?;
        write_tree(
            &repo.objects,
            vec![
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Blob.into(),
                    filename: "README.md".into(),
                    oid: readme_blob,
                },
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: "src".into(),
                    oid: src_tree_oid,
                },
            ],
        )?
    };

    Ok((repo, root_tree_oid))
}

#[test]
fn test_tree_file() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
content = tree.file("README.md")
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_file_nonexistent() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
content = tree.file("nonexistent.txt")
# Should return empty string, not error
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_tree() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
src_tree = tree.tree("src")
main_content = src_tree.file("main.rs")
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_tree_nonexistent() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
nonexistent_tree = tree.tree("nonexistent")
# Should return empty tree, not error
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_dirs() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
dirs_list = tree.dirs("")
# Should contain "src"
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_files() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
files_list = tree.files("")
# Should contain "README.md"
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_nested_access() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
src_tree = tree.tree("src")
main_content = src_tree.file("main.rs")
filter = filter.subdir("src")
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_build_filter_from_all_files() -> anyhow::Result<()> {
    let (repo, root_tree_oid) = create_test_repo()?;

    let script = r#"
def collect_all_files(dir_path=""):
    """Recursively collect all files and build filters"""
    filters = []
    # Get files in current directory
    for file_path in tree.files(dir_path):
        filters.append(filter.file(file_path))
    # Get subdirectories and recurse
    for subdir_path in tree.dirs(dir_path):
        filters.extend(collect_all_files(subdir_path))
    return filters

# Collect all files and compose them
all_file_filters = collect_all_files("")
filter = compose(all_file_filters)
"#;

    let filter = evaluate(script, root_tree_oid, &repo.objects)?;
    let filter_spec = spec(filter);

    // The filter should contain all files: README.md, src/main.rs, src/lib/utils.rs.
    // File filters with multi-component paths decompose into Subdir + File + Prefix
    // chains; common Subdir/Prefix layers around `src` and `lib` get factored out.
    assert_eq!(
        filter_spec,
        ":[::README.md,:/src:[:/lib::utils.rs:prefix=lib,::main.rs]:prefix=src]"
    );
    Ok(())
}
