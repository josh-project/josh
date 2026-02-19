use crate::evaluate::evaluate;
use josh_filter::spec;
use std::sync::{Arc, Mutex};

#[test]
fn test_simple_filter() -> anyhow::Result<()> {
    // Create a temporary repository for testing
    let temp_dir = std::env::temp_dir().join("josh_starlark_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = git2::Repository::init(&temp_dir)?;
    let empty_tree_oid = git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;
    let repo_arc = Arc::new(Mutex::new(repo));

    let script = r#"
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, empty_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_chain_filter() -> anyhow::Result<()> {
    // Create a temporary repository for testing
    let temp_dir = std::env::temp_dir().join("josh_starlark_test2");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = git2::Repository::init(&temp_dir)?;
    let empty_tree_oid = git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;
    let repo_arc = Arc::new(Mutex::new(repo));

    let script = r#"
filter = filter.subdir("src").prefix("lib")
"#;
    let filter = evaluate(script, empty_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src:prefix=lib");
    Ok(())
}

#[test]
fn test_file_filter() -> anyhow::Result<()> {
    // Create a temporary repository for testing
    let temp_dir = std::env::temp_dir().join("josh_starlark_test3");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = git2::Repository::init(&temp_dir)?;
    let empty_tree_oid = git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;
    let repo_arc = Arc::new(Mutex::new(repo));

    let script = r#"
filter = filter.file("README.md")
"#;
    let filter = evaluate(script, empty_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    // file() creates a rename from the same path to itself, which is represented as ::README.md
    assert_eq!(filter_spec, "::README.md");
    Ok(())
}

#[test]
fn test_compose() -> anyhow::Result<()> {
    // Create a temporary repository for testing
    let temp_dir = std::env::temp_dir().join("josh_starlark_test4");
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = git2::Repository::init(&temp_dir)?;
    let empty_tree_oid = git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?;
    let repo_arc = Arc::new(Mutex::new(repo));

    let script = r#"
f1 = filter.subdir("src")
f2 = filter.subdir("lib")
filter = compose([f1, f2])
"#;
    let filter = evaluate(script, empty_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    // compose formats as :[filter1,filter2]
    assert_eq!(filter_spec, ":[:/src,:/lib]");
    Ok(())
}

// Helper function to create a test repository with files and directories
fn create_test_repo() -> anyhow::Result<(Arc<Mutex<git2::Repository>>, git2::Oid)> {
    let temp_dir = std::env::temp_dir().join(format!(
        "josh_starlark_tree_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let repo = git2::Repository::init(&temp_dir)?;
    let repo_arc = Arc::new(Mutex::new(repo));

    // Create a tree with:
    // - README.md (blob)
    // - src/ (tree)
    //   - main.rs (blob)
    //   - lib/ (tree)
    //     - utils.rs (blob)

    let lib_tree_oid = {
        let repo_guard = repo_arc.lock().unwrap();
        // Create blobs
        let utils_rs_blob = repo_guard.blob(b"pub fn helper() {\n    // helper\n}")?;

        // Create nested tree (lib/)
        let mut lib_builder = repo_guard.treebuilder(None)?;
        lib_builder.insert("utils.rs", utils_rs_blob, 0o100644)?;
        lib_builder.write()?
    };

    let src_tree_oid = {
        let repo_guard = repo_arc.lock().unwrap();
        let main_rs_blob = repo_guard.blob(b"fn main() {\n    println!(\"Hello\");\n}")?;

        // Create src/ tree
        let mut src_builder = repo_guard.treebuilder(None)?;
        src_builder.insert("main.rs", main_rs_blob, 0o100644)?;
        src_builder.insert("lib", lib_tree_oid, 0o040000)?;
        src_builder.write()?
    };

    let root_tree_oid = {
        let repo_guard = repo_arc.lock().unwrap();
        let readme_blob = repo_guard.blob(b"# Project\nThis is a test project.")?;

        // Create root tree
        let mut root_builder = repo_guard.treebuilder(None)?;
        root_builder.insert("README.md", readme_blob, 0o100644)?;
        root_builder.insert("src", src_tree_oid, 0o040000)?;
        root_builder.write()?
    };

    Ok((repo_arc, root_tree_oid))
}

#[test]
fn test_tree_file() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test accessing file content
    let script = r#"
content = tree.file("README.md")
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_file_nonexistent() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test accessing non-existent file returns empty string
    let script = r#"
content = tree.file("nonexistent.txt")
# Should return empty string, not error
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_tree() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test accessing a tree
    let script = r#"
src_tree = tree.tree("src")
main_content = src_tree.file("main.rs")
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_tree_nonexistent() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test accessing non-existent tree returns empty tree
    let script = r#"
nonexistent_tree = tree.tree("nonexistent")
# Should return empty tree, not error
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_dirs() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test getting list of directories
    let script = r#"
dirs_list = tree.dirs("")
# Should contain "src"
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_files() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test getting list of files
    let script = r#"
files_list = tree.files("")
# Should contain "README.md"
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_nested_access() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test nested access: tree.tree("src").file("main.rs")
    let script = r#"
src_tree = tree.tree("src")
main_content = src_tree.file("main.rs")
filter = filter.subdir("src")
"#;
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);
    assert_eq!(filter_spec, ":/src");
    Ok(())
}

#[test]
fn test_tree_build_filter_from_all_files() -> anyhow::Result<()> {
    let (repo_arc, root_tree_oid) = create_test_repo()?;

    // Test building a filter that includes all files from the tree
    // Using the new API: tree.files() to get file lists
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
    let filter = evaluate(script, root_tree_oid, repo_arc.clone())?;
    let filter_spec = spec(filter);

    // The filter should contain all files: README.md, src/main.rs, src/lib/utils.rs
    // Compose of file filters (sorted alphabetically): :[::README.md,::src/lib/utils.rs,::src/main.rs]
    assert_eq!(
        filter_spec,
        ":[::README.md,::src/lib/utils.rs,::src/main.rs]"
    );
    Ok(())
}
