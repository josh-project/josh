//! Prints what serialized values look like as git trees.
//!
//! Every value is converted with [`to_value`], then rendered through
//! `GitValue`'s `Debug`: blobs print their contents (truncated past 64
//! bytes), trees print their entries -- the exact filenames the objects
//! get on disk, including percent-encoded map keys.
//!
//! Run with `cargo run -p josh-git-serde --example representation`.

use josh_git_serde::to_value;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct CommitMeta {
    author: String,
    committed_at: u64,
    verified: bool,
}

#[derive(Serialize)]
enum Change {
    Created,
    Renamed(String),
    Modified { old_mode: u32, new_mode: u32 },
}

#[derive(Serialize)]
struct PullRequest {
    id: u64,
    title: String,
    milestone: Option<String>,
    meta: Option<CommitMeta>,
    change: Change,
    files: BTreeMap<String, String>,
}

fn section(name: &str) {
    println!("\n== {name} ==");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    section("scalars");
    println!("{:?}", to_value(&42u64)?);
    println!("{:?}", to_value("hello")?);
    println!("{:?}", to_value(&true)?);

    section("struct");
    let meta = CommitMeta {
        author: "josh".to_string(),
        committed_at: 1_724_000_000,
        verified: true,
    };
    println!("{:#?}", to_value(&meta)?);

    section("map with filename-unsafe keys");
    let mut files = BTreeMap::new();
    files.insert("src/main.rs".to_string(), "...".to_string());
    files.insert("a b.txt".to_string(), "...".to_string());
    println!("{:#?}", to_value(&files)?);

    section("enum variants");
    println!("{:#?}", to_value(&Change::Created)?);
    println!("{:#?}", to_value(&Change::Renamed("old.rs".into()))?);
    println!(
        "{:#?}",
        to_value(&Change::Modified {
            old_mode: 0o100644,
            new_mode: 0o100755,
        })?
    );

    section("nested composite");
    let pr = PullRequest {
        id: 7,
        title: "Add tree serde".to_string(),
        milestone: Some("v2".to_string()),
        meta: Some(meta),
        change: Change::Modified {
            old_mode: 0o100644,
            new_mode: 0o100755,
        },
        files,
    };
    println!("{:#?}", to_value(&pr)?);

    section("nested composite, `None` field omitted");
    let pr_without_milestone = PullRequest {
        id: 8,
        title: "Drop markers".to_string(),
        milestone: None,
        meta: None,
        change: Change::Created,
        files: BTreeMap::new(),
    };
    println!("{:#?}", to_value(&pr_without_milestone)?);

    Ok(())
}
