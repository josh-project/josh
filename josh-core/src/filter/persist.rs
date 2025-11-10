use crate::filter::{Filter, LazyRef, Op, to_filter, to_op};
use crate::{JoshResult, josh_error};

pub fn as_tree(repo: &git2::Repository, filter: Filter) -> JoshResult<git2::Oid> {
    as_tree2(repo, &to_op(filter))
}

fn filter_params(repo: &git2::Repository, params: &[Filter]) -> JoshResult<git2::Oid> {
    let mut builder = repo.treebuilder(None)?;
    for (i, f) in params.iter().enumerate() {
        let child = as_tree(repo, *f)?;
        builder.insert(format!("{}", i), child, git2::FileMode::Tree.into())?;
    }
    Ok(builder.write()?)
}

fn str_params(repo: &git2::Repository, params: &[&str]) -> JoshResult<git2::Oid> {
    let mut builder = repo.treebuilder(None)?;
    for (i, f) in params.iter().enumerate() {
        builder.insert(
            format!("{}", i),
            repo.blob(f.as_bytes())?,
            git2::FileMode::Blob.into(),
        )?;
    }
    Ok(builder.write()?)
}

fn as_tree2(repo: &git2::Repository, op: &Op) -> JoshResult<git2::Oid> {
    let mut builder = repo.treebuilder(None)?;
    match op {
        Op::Author(name, email) => {
            builder.insert(
                "author",
                str_params(repo, &[name, email])?,
                git2::FileMode::Tree.into(),
            )?;
        }
        Op::Committer(name, email) => {
            builder.insert(
                "committer",
                str_params(repo, &[name, email])?,
                git2::FileMode::Tree.into(),
            )?;
        }
        Op::Compose(filters) => {
            builder.insert(
                "compose",
                filter_params(repo, filters)?,
                git2::FileMode::Tree.into(),
            )?;
        }
        Op::Subtract(a, b) => {
            builder.insert(
                "subtract",
                filter_params(repo, &[*a, *b])?,
                git2::FileMode::Tree.into(),
            )?;
        }
        Op::Chain(a, b) => {
            builder.insert(
                "chain",
                filter_params(repo, &[*a, *b])?,
                git2::FileMode::Tree.into(),
            )?;
        }
        Op::Exclude(b) => {
            builder.insert("exclude", as_tree(repo, *b)?, git2::FileMode::Tree.into())?;
        }
        Op::Pin(b) => {
            builder.insert("pin", as_tree(repo, *b)?, git2::FileMode::Tree.into())?;
        }
        Op::Subdir(path) => {
            builder.insert(
                "subdir",
                repo.blob(&path.to_string_lossy().as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Prefix(path) => {
            builder.insert(
                "prefix",
                repo.blob(&path.to_string_lossy().as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::File(path) => {
            builder.insert(
                "file",
                repo.blob(&path.to_string_lossy().as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Pattern(pattern) => {
            builder.insert(
                "pattern",
                repo.blob(&pattern.as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Message(m) => {
            builder.insert(
                "message",
                repo.blob(&m.as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Workspace(path) => {
            builder.insert(
                "workspace",
                repo.blob(&path.to_string_lossy().as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Nop => {
            builder.insert(
                "nop",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Empty => {
            builder.insert(
                "empty",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Paths => {
            builder.insert(
                "paths",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Invert => {
            builder.insert(
                "invert",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Index => {
            builder.insert(
                "index",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Fold => {
            builder.insert(
                "fold",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Linear => {
            builder.insert(
                "linear",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Unsign => {
            builder.insert(
                "unsign",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Squash(None) => {
            builder.insert(
                "squash",
                repo.blob(&"".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Prune => {
            builder.insert(
                "prune",
                repo.blob(&"trivial-merge".as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
        Op::Rev(filters) => {
            let mut v = filters
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<Vec<_>>();
            v.sort();
            builder.insert("rev", rev_params(repo, &v)?, git2::FileMode::Tree.into())?;
        }
        Op::Join(filters) => {
            let mut v = filters
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<Vec<_>>();
            v.sort();
            builder.insert("join", rev_params(repo, &v)?, git2::FileMode::Tree.into())?;
        }
        Op::Squash(Some(ids)) => {
            let mut v = ids
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<Vec<_>>();
            v.sort();
            builder.insert("squash", rev_params(repo, &v)?, git2::FileMode::Tree.into())?;
        }
        Op::RegexReplace(replacements) => {
            builder.insert(
                "regex_replace",
                regex_replace_params(repo, replacements)?,
                git2::FileMode::Tree.into(),
            )?;
        }
        Op::Hook(hook) => {
            builder.insert(
                "hook",
                repo.blob(hook.as_bytes())?,
                git2::FileMode::Blob.into(),
            )?;
        }
    };
    Ok(builder.write()?)
}

fn rev_params(repo: &git2::Repository, params: &[(String, Filter)]) -> JoshResult<git2::Oid> {
    let mut outer = repo.treebuilder(None)?;
    for (i, (key, filter)) in params.iter().enumerate() {
        let mut inner = repo.treebuilder(None)?;
        inner.insert("o", repo.blob(key.as_bytes())?, git2::FileMode::Blob.into())?;
        inner.insert("f", as_tree(repo, *filter)?, git2::FileMode::Tree.into())?;
        let inner_oid = inner.write()?;
        outer.insert(format!("{}", i), inner_oid, git2::FileMode::Tree.into())?;
    }
    Ok(outer.write()?)
}

fn regex_replace_params(
    repo: &git2::Repository,
    replacements: &[(regex::Regex, String)],
) -> JoshResult<git2::Oid> {
    let mut outer = repo.treebuilder(None)?;
    for (i, (regex, replacement)) in replacements.iter().enumerate() {
        let mut inner = repo.treebuilder(None)?;
        inner.insert(
            "p",
            repo.blob(regex.as_str().as_bytes())?,
            git2::FileMode::Blob.into(),
        )?;
        inner.insert(
            "r",
            repo.blob(replacement.as_bytes())?,
            git2::FileMode::Blob.into(),
        )?;
        let inner_oid = inner.write()?;
        outer.insert(format!("{}", i), inner_oid, git2::FileMode::Tree.into())?;
    }
    Ok(outer.write()?)
}

pub fn from_tree(repo: &git2::Repository, tree_oid: git2::Oid) -> JoshResult<Filter> {
    Ok(to_filter(from_tree2(repo, tree_oid)?))
}
fn from_tree2(repo: &git2::Repository, tree_oid: git2::Oid) -> JoshResult<Op> {
    let tree = repo.find_tree(tree_oid)?;

    // Assume there's only one entry and get it directly
    let entry = tree.get(0).ok_or_else(|| josh_error("Empty tree"))?;
    let name = entry
        .name()
        .ok_or_else(|| josh_error("Entry has no name"))?;

    match name {
        "nop" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Nop)
        }
        "empty" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Empty)
        }
        "paths" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Paths)
        }
        "invert" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Invert)
        }
        "index" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Index)
        }
        "fold" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Fold)
        }
        "linear" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Linear)
        }
        "unsign" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Unsign)
        }
        "prune" => {
            let blob = repo.find_blob(entry.id())?;
            let content = std::str::from_utf8(blob.content())?;
            if content == "trivial-merge" {
                Ok(Op::Prune)
            } else {
                Err(josh_error("Invalid prune content"))
            }
        }
        "hook" => {
            let blob = repo.find_blob(entry.id())?;
            let hook_name = std::str::from_utf8(blob.content())?.to_string();
            Ok(Op::Hook(hook_name))
        }
        "author" => {
            let inner = repo.find_tree(entry.id())?;
            let name_blob = repo.find_blob(
                inner
                    .get_name("0")
                    .ok_or_else(|| josh_error("author: missing name"))?
                    .id(),
            )?;
            let email_blob = repo.find_blob(
                inner
                    .get_name("1")
                    .ok_or_else(|| josh_error("author: missing email"))?
                    .id(),
            )?;
            let name = std::str::from_utf8(name_blob.content())?.to_string();
            let email = std::str::from_utf8(email_blob.content())?.to_string();
            Ok(Op::Author(name, email))
        }
        "committer" => {
            let inner = repo.find_tree(entry.id())?;
            let name_blob = repo.find_blob(
                inner
                    .get_name("0")
                    .ok_or_else(|| josh_error("committer: missing name"))?
                    .id(),
            )?;
            let email_blob = repo.find_blob(
                inner
                    .get_name("1")
                    .ok_or_else(|| josh_error("committer: missing email"))?
                    .id(),
            )?;
            let name = std::str::from_utf8(name_blob.content())?.to_string();
            let email = std::str::from_utf8(email_blob.content())?.to_string();
            Ok(Op::Committer(name, email))
        }
        "subdir" => {
            let blob = repo.find_blob(entry.id())?;
            let path = std::str::from_utf8(blob.content())?;
            Ok(Op::Subdir(std::path::PathBuf::from(path)))
        }
        "prefix" => {
            let blob = repo.find_blob(entry.id())?;
            let path = std::str::from_utf8(blob.content())?;
            Ok(Op::Prefix(std::path::PathBuf::from(path)))
        }
        "file" => {
            let blob = repo.find_blob(entry.id())?;
            let path = std::str::from_utf8(blob.content())?;
            Ok(Op::File(std::path::PathBuf::from(path)))
        }
        "pattern" => {
            let blob = repo.find_blob(entry.id())?;
            let pattern = std::str::from_utf8(blob.content())?.to_string();
            Ok(Op::Pattern(pattern))
        }
        "message" => {
            let blob = repo.find_blob(entry.id())?;
            let message = std::str::from_utf8(blob.content())?.to_string();
            Ok(Op::Message(message))
        }
        "workspace" => {
            let blob = repo.find_blob(entry.id())?;
            let path = std::str::from_utf8(blob.content())?;
            Ok(Op::Workspace(std::path::PathBuf::from(path)))
        }
        "compose" => {
            let compose_tree = repo.find_tree(entry.id())?;
            let mut filters = Vec::new();
            for i in 0..compose_tree.len() {
                let entry = compose_tree
                    .get(i)
                    .ok_or_else(|| josh_error("compose: missing entry"))?;
                let filter_tree = repo.find_tree(entry.id())?;
                let filter = from_tree2(repo, filter_tree.id())?;
                filters.push(to_filter(filter));
            }
            Ok(Op::Compose(filters))
        }
        "subtract" => {
            let subtract_tree = repo.find_tree(entry.id())?;
            if subtract_tree.len() == 2 {
                let a_tree = repo.find_tree(
                    subtract_tree
                        .get_name("0")
                        .ok_or_else(|| josh_error("subtract: missing 0"))?
                        .id(),
                )?;
                let b_tree = repo.find_tree(
                    subtract_tree
                        .get_name("1")
                        .ok_or_else(|| josh_error("subtract: missing 1"))?
                        .id(),
                )?;
                let a = from_tree2(repo, a_tree.id())?;
                let b = from_tree2(repo, b_tree.id())?;
                Ok(Op::Subtract(to_filter(a), to_filter(b)))
            } else {
                Err(josh_error("subtract: expected 2 entries"))
            }
        }
        "chain" => {
            let chain_tree = repo.find_tree(entry.id())?;
            if chain_tree.len() == 2 {
                let a_tree = repo.find_tree(
                    chain_tree
                        .get_name("0")
                        .ok_or_else(|| josh_error("chain: missing 0"))?
                        .id(),
                )?;
                let b_tree = repo.find_tree(
                    chain_tree
                        .get_name("1")
                        .ok_or_else(|| josh_error("chain: missing 1"))?
                        .id(),
                )?;
                let a = from_tree2(repo, a_tree.id())?;
                let b = from_tree2(repo, b_tree.id())?;
                Ok(Op::Chain(to_filter(a), to_filter(b)))
            } else {
                Err(josh_error("chain: expected 2 entries"))
            }
        }
        "exclude" => {
            let exclude_tree = repo.find_tree(entry.id())?;
            let filter = from_tree2(repo, exclude_tree.id())?;
            Ok(Op::Exclude(to_filter(filter)))
        }
        "pin" => {
            let pin_tree = repo.find_tree(entry.id())?;
            let filter = from_tree2(repo, pin_tree.id())?;
            Ok(Op::Pin(to_filter(filter)))
        }
        "rev" => {
            let rev_tree = repo.find_tree(entry.id())?;
            let mut filters = std::collections::BTreeMap::new();
            for i in 0..rev_tree.len() {
                let entry = rev_tree
                    .get(i)
                    .ok_or_else(|| josh_error("rev: missing entry"))?;
                let inner_tree = repo.find_tree(entry.id())?;
                let key_blob = repo.find_blob(
                    inner_tree
                        .get_name("o")
                        .ok_or_else(|| josh_error("rev: missing key"))?
                        .id(),
                )?;
                let filter_tree = repo.find_tree(
                    inner_tree
                        .get_name("f")
                        .ok_or_else(|| josh_error("rev: missing filter"))?
                        .id(),
                )?;
                let key = std::str::from_utf8(key_blob.content())?.to_string();
                let filter = from_tree2(repo, filter_tree.id())?;
                filters.insert(LazyRef::parse(&key)?, to_filter(filter));
            }
            Ok(Op::Rev(filters))
        }
        "join" => {
            let join_tree = repo.find_tree(entry.id())?;
            let mut filters = std::collections::BTreeMap::new();
            for i in 0..join_tree.len() {
                let entry = join_tree
                    .get(i)
                    .ok_or_else(|| josh_error("join: missing entry"))?;
                let inner_tree = repo.find_tree(entry.id())?;
                let key_blob = repo.find_blob(
                    inner_tree
                        .get_name("o")
                        .ok_or_else(|| josh_error("join: missing key"))?
                        .id(),
                )?;
                let filter_tree = repo.find_tree(
                    inner_tree
                        .get_name("f")
                        .ok_or_else(|| josh_error("join: missing filter"))?
                        .id(),
                )?;
                let key = std::str::from_utf8(key_blob.content())?.to_string();
                let filter = from_tree2(repo, filter_tree.id())?;
                filters.insert(LazyRef::parse(&key)?, to_filter(filter));
            }
            Ok(Op::Join(filters))
        }
        "squash" => {
            // blob -> Squash(None), tree -> Squash(Some(...))
            if let Some(kind) = entry.kind() {
                if kind == git2::ObjectType::Blob {
                    let _ = repo.find_blob(entry.id())?;
                    return Ok(Op::Squash(None));
                }
            }
            let squash_tree = repo.find_tree(entry.id())?;
            let mut filters = std::collections::BTreeMap::new();
            for i in 0..squash_tree.len() {
                let entry = squash_tree
                    .get(i)
                    .ok_or_else(|| josh_error("squash: missing entry"))?;
                let inner_tree = repo.find_tree(entry.id())?;
                let key_blob = repo.find_blob(
                    inner_tree
                        .get_name("o")
                        .ok_or_else(|| josh_error("squash: missing key"))?
                        .id(),
                )?;
                let filter_tree = repo.find_tree(
                    inner_tree
                        .get_name("f")
                        .ok_or_else(|| josh_error("squash: missing filter"))?
                        .id(),
                )?;
                let key = std::str::from_utf8(key_blob.content())?.to_string();
                let filter = from_tree2(repo, filter_tree.id())?;
                filters.insert(LazyRef::parse(&key)?, to_filter(filter));
            }
            Ok(Op::Squash(Some(filters)))
        }
        "regex_replace" => {
            let regex_replace_tree = repo.find_tree(entry.id())?;
            let mut replacements = Vec::new();
            for i in 0..regex_replace_tree.len() {
                let entry = regex_replace_tree
                    .get(i)
                    .ok_or_else(|| josh_error("regex_replace: missing entry"))?;
                let inner_tree = repo.find_tree(entry.id())?;
                let regex_blob = repo.find_blob(
                    inner_tree
                        .get_name("p")
                        .ok_or_else(|| josh_error("regex_replace: missing pattern"))?
                        .id(),
                )?;
                let replacement_blob = repo.find_blob(
                    inner_tree
                        .get_name("r")
                        .ok_or_else(|| josh_error("regex_replace: missing replacement"))?
                        .id(),
                )?;
                let regex_str = std::str::from_utf8(regex_blob.content())?;
                let replacement = std::str::from_utf8(replacement_blob.content())?.to_string();
                let regex = regex::Regex::new(regex_str)?;
                replacements.push((regex, replacement));
            }
            Ok(Op::RegexReplace(replacements))
        }
        _ => Err(josh_error("Unknown tree structure")),
    }
}
