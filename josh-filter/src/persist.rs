use anyhow::{Context, anyhow};
use gix_object::WriteTo;
use gix_object::bstr::BString;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::LazyLock;

use crate::filter::{Filter, sequence_number};
use crate::hash::PassthroughHasher;
use crate::op::{LazyRef, Op, RevMatch};

pub(crate) static FILTERS: LazyLock<
    std::sync::Mutex<HashMap<Filter, Op, BuildHasherDefault<PassthroughHasher>>>,
> = LazyLock::new(Default::default);

pub fn peel_op(filter: Filter) -> Op {
    let op = to_op(filter);
    if let Op::Meta(_, f) = op {
        peel_op(f)
    } else {
        op
    }
}

pub fn to_op(filter: Filter) -> Op {
    if filter == sequence_number() {
        return Op::Nop;
    }
    FILTERS
        .lock()
        .unwrap()
        .get(&filter)
        .expect("unknown filter")
        .clone()
}

pub fn to_ops(filters: &[Filter]) -> Vec<Op> {
    filters.iter().map(|x| to_op(*x)).collect()
}

/// Get a clone of the FILTERS map for use in as_tree/from_tree
pub fn get_filters() -> HashMap<Filter, Op, BuildHasherDefault<PassthroughHasher>> {
    FILTERS.lock().unwrap().clone()
}

fn push_blob_entries(
    entries: &mut Vec<gix_object::tree::Entry>,
    items: impl IntoIterator<Item = (impl AsRef<str>, gix_hash::ObjectId)>,
) {
    for (name, oid) in items {
        entries.push(gix_object::tree::Entry {
            mode: gix_object::tree::EntryKind::Blob.into(),
            filename: BString::from(name.as_ref()),
            oid,
        });
    }
}

fn push_tree_entries(
    entries: &mut Vec<gix_object::tree::Entry>,
    items: impl IntoIterator<Item = (impl AsRef<str>, gix_hash::ObjectId)>,
) {
    for (name, oid) in items {
        entries.push(gix_object::tree::Entry {
            mode: gix_object::tree::EntryKind::Tree.into(),
            filename: BString::from(name.as_ref()),
            oid,
        });
    }
}

struct InMemoryBuilder {
    // Map from hash to (kind, raw bytes)
    pending_writes: HashMap<gix_hash::ObjectId, (gix_object::Kind, Vec<u8>)>,
}

impl InMemoryBuilder {
    fn new() -> Self {
        // Add an empty blob because we use a shortcut for them below
        // in write_blob
        let mut pending_writes = HashMap::new();
        pending_writes.insert(
            gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1),
            (gix_object::Kind::Blob, Vec::new()),
        );

        Self { pending_writes }
    }

    fn write_blob(&mut self, data: &[u8]) -> gix_hash::ObjectId {
        if data.is_empty() {
            return gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1);
        }

        let hash = gix_object::compute_hash(gix_hash::Kind::Sha1, gix_object::Kind::Blob, data)
            .expect("failed to compute hash");
        self.pending_writes
            .insert(hash, (gix_object::Kind::Blob, data.to_vec()));
        hash
    }

    fn write_tree(&mut self, mut tree: gix_object::Tree) -> gix_hash::ObjectId {
        tree.entries.sort_by(|a, b| a.filename.cmp(&b.filename));
        let mut buffer = Vec::with_capacity(tree.size() as usize);
        tree.write_to(&mut buffer).expect("failed to write tree");
        let hash = gix_object::compute_hash(gix_hash::Kind::Sha1, gix_object::Kind::Tree, &buffer)
            .expect("failed to compute hash");
        self.pending_writes
            .insert(hash, (gix_object::Kind::Tree, buffer));
        hash
    }

    fn build_str_params(&mut self, params: &[&str]) -> gix_hash::ObjectId {
        let mut entries = Vec::new();

        let indexed_blobs: Vec<_> = params
            .iter()
            .enumerate()
            .map(|(i, param)| (i.to_string(), self.write_blob(param.as_bytes())))
            .collect();
        push_blob_entries(&mut entries, indexed_blobs);

        let tree = gix_object::Tree { entries };
        self.write_tree(tree)
    }

    fn build_filter_params(&mut self, params: &[Filter]) -> anyhow::Result<gix_hash::ObjectId> {
        let mut entries = Vec::new();
        for (i, filter) in params.iter().enumerate() {
            let child = gix_hash::ObjectId::from_bytes_or_panic(filter.id().as_bytes());
            entries.push(gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from(i.to_string()),
                oid: child,
            });
        }
        let tree = gix_object::Tree { entries };
        Ok(self.write_tree(tree))
    }

    #[cfg(feature = "incubating")]
    fn build_starlark_params(
        &mut self,
        path: &std::path::Path,
        subfilter: Filter,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let path_tree = self.build_str_params(&[path.to_string_lossy().as_ref()]);
        let filter_tree = self.build_filter_params(&[subfilter])?;
        let entries = vec![
            gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from("0"),
                oid: path_tree,
            },
            gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from("1"),
                oid: filter_tree,
            },
        ];
        Ok(self.write_tree(gix_object::Tree { entries }))
    }

    fn build_rev_params(
        &mut self,
        params: &[(RevMatch, LazyRef, Filter)],
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let mut outer_entries = Vec::new();
        for (i, (match_op, lazy_ref, filter)) in params.iter().enumerate() {
            // Encode match operator as prefix
            let key = match match_op {
                RevMatch::AncestorStrict => format!("<{}", lazy_ref),
                RevMatch::AncestorInclusive => format!("<={}", lazy_ref),
                RevMatch::Equal => format!("=={}", lazy_ref),
                RevMatch::Default => {
                    // Default filter uses "_" as key (no SHA)
                    "_".to_string()
                }
            };
            let key_blob = self.write_blob(key.as_bytes());
            let filter_tree = gix_hash::ObjectId::from_bytes_or_panic(filter.id().as_bytes());

            let inner_entries = vec![
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Blob.into(),
                    filename: BString::from("o"),
                    oid: key_blob,
                },
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: BString::from("f"),
                    oid: filter_tree,
                },
            ];
            let inner_tree = gix_object::Tree {
                entries: inner_entries,
            };
            let inner_oid = self.write_tree(inner_tree);

            outer_entries.push(gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from(i.to_string()),
                oid: inner_oid,
            });
        }
        let outer_tree = gix_object::Tree {
            entries: outer_entries,
        };
        Ok(self.write_tree(outer_tree))
    }

    #[cfg(feature = "incubating")]
    fn build_lazyref_filter_params(
        &mut self,
        lazy_ref: &LazyRef,
        filter: Filter,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let key_blob = self.write_blob(lazy_ref.to_string().as_bytes());
        let filter_tree = gix_hash::ObjectId::from_bytes_or_panic(filter.id().as_bytes());

        let inner_entries = vec![
            gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Blob.into(),
                filename: BString::from("o"),
                oid: key_blob,
            },
            gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from("f"),
                oid: filter_tree,
            },
        ];
        let inner_tree = gix_object::Tree {
            entries: inner_entries,
        };
        let inner_oid = self.write_tree(inner_tree);

        let outer_entries = vec![gix_object::tree::Entry {
            mode: gix_object::tree::EntryKind::Tree.into(),
            filename: BString::from("0"),
            oid: inner_oid,
        }];
        let outer_tree = gix_object::Tree {
            entries: outer_entries,
        };
        Ok(self.write_tree(outer_tree))
    }

    fn build_squash_params(
        &mut self,
        params: &std::collections::BTreeMap<LazyRef, Filter>,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let mut outer_entries = Vec::new();
        for (i, (lazy_ref, filter)) in params.iter().enumerate() {
            let key_blob = self.write_blob(lazy_ref.to_string().as_bytes());
            let filter_tree = gix_hash::ObjectId::from_bytes_or_panic(filter.id().as_bytes());

            let inner_entries = vec![
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Blob.into(),
                    filename: BString::from("o"),
                    oid: key_blob,
                },
                gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: BString::from("f"),
                    oid: filter_tree,
                },
            ];
            let inner_tree = gix_object::Tree {
                entries: inner_entries,
            };
            let inner_oid = self.write_tree(inner_tree);

            outer_entries.push(gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from(i.to_string()),
                oid: inner_oid,
            });
        }
        let outer_tree = gix_object::Tree {
            entries: outer_entries,
        };
        Ok(self.write_tree(outer_tree))
    }

    fn build_regex_replace_params(
        &mut self,
        replacements: &[(regex::Regex, String)],
    ) -> gix_hash::ObjectId {
        let mut outer_entries = Vec::new();
        for (i, (regex, replacement)) in replacements.iter().enumerate() {
            let regex_blob = self.write_blob(regex.as_str().as_bytes());
            let replacement_blob = self.write_blob(replacement.as_bytes());

            let mut inner_entries = Vec::new();
            push_blob_entries(
                &mut inner_entries,
                [("p", regex_blob), ("r", replacement_blob)],
            );
            let inner_tree = gix_object::Tree {
                entries: inner_entries,
            };
            let inner_oid = self.write_tree(inner_tree);

            outer_entries.push(gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Tree.into(),
                filename: BString::from(format!("{}", i)),
                oid: inner_oid,
            });
        }
        let outer_tree = gix_object::Tree {
            entries: outer_entries,
        };
        self.write_tree(outer_tree)
    }

    fn build_op(&mut self, op: &Op) -> anyhow::Result<gix_hash::ObjectId> {
        let mut entries = Vec::new();

        match op {
            Op::Message(fmt, regex) => {
                let params_tree = self.build_str_params(&[fmt, regex.as_str()]);
                push_tree_entries(&mut entries, [("message", params_tree)]);
            }
            Op::Author(name, email) => {
                let params_tree = self.build_str_params(&[name, email]);
                push_tree_entries(&mut entries, [("author", params_tree)]);
            }
            Op::Committer(name, email) => {
                let params_tree = self.build_str_params(&[name, email]);
                push_tree_entries(&mut entries, [("committer", params_tree)]);
            }
            Op::Compose(filters) => {
                let params_tree = self.build_filter_params(filters)?;
                push_tree_entries(&mut entries, [("compose", params_tree)]);
            }
            Op::Subtract(a, b) => {
                let params_tree = self.build_filter_params(&[*a, *b])?;
                push_tree_entries(&mut entries, [("subtract", params_tree)]);
            }
            Op::Chain(filters) => {
                let params_tree = self.build_filter_params(filters)?;
                push_tree_entries(&mut entries, [("chain", params_tree)]);
            }
            Op::Exclude(b) => {
                let params_tree = self.build_filter_params(&[*b])?;
                push_tree_entries(&mut entries, [("exclude", params_tree)]);
            }
            Op::Pin(b) => {
                let params_tree = self.build_filter_params(&[*b])?;
                push_tree_entries(&mut entries, [("pin", params_tree)]);
            }
            Op::Subdir(path) => {
                let params_tree = self.build_str_params(&[path.to_string_lossy().as_ref()]);
                push_tree_entries(&mut entries, [("subdir", params_tree)]);
            }
            Op::Prefix(path) => {
                let params_tree = self.build_str_params(&[path.to_string_lossy().as_ref()]);
                push_tree_entries(&mut entries, [("prefix", params_tree)]);
            }
            Op::File(dest_path, source_path) => {
                // Store as (dest_path, source_path) to match enum order
                let params_tree = self.build_str_params(&[
                    dest_path.to_string_lossy().as_ref(),
                    source_path.to_string_lossy().as_ref(),
                ]);
                push_tree_entries(&mut entries, [("file", params_tree)]);
            }
            #[cfg(feature = "incubating")]
            Op::Embed(path) => {
                let params_tree = self.build_str_params(&[path.to_string_lossy().as_ref()]);
                push_tree_entries(&mut entries, [("embed", params_tree)]);
            }
            Op::Pattern(pattern) => {
                let params_tree = self.build_str_params(&[pattern.as_ref()]);
                push_tree_entries(&mut entries, [("pattern", params_tree)]);
            }
            Op::Workspace(path) => {
                let params_tree = self.build_str_params(&[path.to_string_lossy().as_ref()]);
                push_tree_entries(&mut entries, [("workspace", params_tree)]);
            }
            Op::Stored(path) => {
                let params_tree = self.build_str_params(&[path.to_string_lossy().as_ref()]);
                push_tree_entries(&mut entries, [("stored", params_tree)]);
            }
            #[cfg(feature = "incubating")]
            Op::Starlark(path, subfilter) => {
                let params_tree = self.build_starlark_params(path, *subfilter)?;
                push_tree_entries(&mut entries, [("starlark", params_tree)]);
            }
            Op::Nop => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("nop", blob)]);
            }
            Op::Empty => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("empty", blob)]);
            }
            #[cfg(feature = "incubating")]
            Op::Export => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("export", blob)]);
            }
            Op::Paths => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("paths", blob)]);
            }
            #[cfg(feature = "incubating")]
            Op::Link(mode) => {
                let mode_str = mode.as_ref().map(|m| m.to_string()).unwrap_or_default();
                let params_tree = self.build_str_params(&[&mode_str]);
                push_tree_entries(&mut entries, [("link", params_tree)]);
            }
            #[cfg(feature = "incubating")]
            Op::Adapt(mode) => {
                let params_tree = self.build_str_params(&[mode.as_ref()]);
                push_tree_entries(&mut entries, [("adapt", params_tree)]);
            }
            #[cfg(feature = "incubating")]
            Op::Unlink => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("unlink", blob)]);
            }
            Op::Invert => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("invert", blob)]);
            }
            Op::Index => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("index", blob)]);
            }
            Op::Fold => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("fold", blob)]);
            }
            Op::Squash(None) => {
                let blob = self.write_blob(b"");
                push_blob_entries(&mut entries, [("squash", blob)]);
            }
            Op::Prune => {
                let blob = self.write_blob(b"trivial-merge");
                push_blob_entries(&mut entries, [("prune", blob)]);
            }
            Op::Rev(filters) => {
                // No sorting - preserve order for first-match semantics
                let params_tree = self.build_rev_params(filters)?;
                push_tree_entries(&mut entries, [("rev", params_tree)]);
            }
            #[cfg(feature = "incubating")]
            Op::Unapply(lr, f) => {
                let params_tree = self.build_lazyref_filter_params(lr, *f)?;
                push_tree_entries(&mut entries, [("unapply", params_tree)]);
            }
            Op::Squash(Some(ids)) => {
                let params_tree = self.build_squash_params(ids)?;
                push_tree_entries(&mut entries, [("squash", params_tree)]);
            }
            Op::RegexReplace(replacements) => {
                let params_tree = self.build_regex_replace_params(replacements);
                push_tree_entries(&mut entries, [("regex_replace", params_tree)]);
            }
            Op::Hook(hook) => {
                let params_tree = self.build_str_params(&[hook.as_ref()]);
                push_tree_entries(&mut entries, [("hook", params_tree)]);
            }
            Op::Meta(meta, filter) => {
                let mut meta_entries = Vec::new();
                for (key, value) in meta.iter() {
                    let value_blob = self.write_blob(value.as_bytes());
                    push_blob_entries(&mut meta_entries, [(key.as_str(), value_blob)]);
                }
                let filter_tree = gix_hash::ObjectId::from_bytes_or_panic(filter.id().as_bytes());
                push_tree_entries(&mut meta_entries, [("0", filter_tree)]);
                let meta_tree = self.write_tree(gix_object::Tree {
                    entries: meta_entries,
                });
                push_tree_entries(&mut entries, [("meta", meta_tree)]);
            }
        }

        let tree = gix_object::Tree { entries };
        Ok(self.write_tree(tree))
    }
}

pub fn to_filter(op: Op) -> Filter {
    let mut builder = InMemoryBuilder::new();
    let tree_id = builder.build_op(&op).expect("failed to build op");
    let oid = git2::Oid::from_bytes(tree_id.as_bytes()).unwrap();

    let f = Filter(oid);
    FILTERS.lock().unwrap().entry(f).or_insert(op);
    f
}

pub fn as_tree(repo: &git2::Repository, filter: Filter) -> anyhow::Result<git2::Oid> {
    let odb = repo.odb()?;

    // If the tree exists in the ODB it means all children must already exist as
    // well so we can just return it.
    if odb.exists(filter.id()) {
        return Ok(filter.id());
    }

    // We don't try to figure out what to write exactly, just write all
    // filters we know about to the ODB
    let filters = get_filters();
    let mut builder = InMemoryBuilder::new();
    for (f, op) in filters.into_iter() {
        if !odb.exists(f.id()) {
            builder.build_op(&op)?;
        }
    }

    // Write all pending objects to the git2 repository
    for (oid, (kind, data)) in builder.pending_writes {
        let oid = git2::Oid::from_bytes(oid.as_bytes())?;

        // On some platforms, .exists() is cheaper in terms of i/o
        // than .write(), because .write() updates file access time
        // in loose object backend
        if !odb.exists(oid) {
            let git2_type = match kind {
                gix_object::Kind::Tree => git2::ObjectType::Tree,
                gix_object::Kind::Blob => git2::ObjectType::Blob,
                gix_object::Kind::Commit => git2::ObjectType::Commit,
                gix_object::Kind::Tag => git2::ObjectType::Tag,
            };
            odb.write(git2_type, &data)?;
        }
    }

    // Now the tree should really be in the ODB
    Ok(filter.id())
}

pub fn from_tree(repo: &git2::Repository, tree_oid: git2::Oid) -> anyhow::Result<Filter> {
    Ok(to_filter(from_tree2(repo, tree_oid)?))
}

fn from_tree2(repo: &git2::Repository, tree_oid: git2::Oid) -> anyhow::Result<Op> {
    let tree = repo.find_tree(tree_oid)?;

    // Assume there's only one entry and get it directly
    let entry = tree.get(0).context("Empty tree")?;
    let name = entry.name().context("Entry has no name")?;

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
        #[cfg(feature = "incubating")]
        "export" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Export)
        }
        #[cfg(feature = "incubating")]
        "link" => {
            let inner = repo.find_tree(entry.id())?;
            let mode_blob =
                repo.find_blob(inner.get_name("0").context("link: missing mode")?.id())?;
            let mode_str = std::str::from_utf8(mode_blob.content())?;
            let mode = if mode_str.is_empty() {
                None
            } else {
                Some(crate::op::LinkMode::parse(mode_str)?)
            };
            Ok(Op::Link(mode))
        }
        #[cfg(feature = "incubating")]
        "adapt" => {
            let inner = repo.find_tree(entry.id())?;
            let mode_blob =
                repo.find_blob(inner.get_name("0").context("adapt: missing mode")?.id())?;
            Ok(Op::Adapt(
                std::str::from_utf8(mode_blob.content())?.to_string(),
            ))
        }
        #[cfg(feature = "incubating")]
        "unlink" => {
            let _ = repo.find_blob(entry.id())?;
            Ok(Op::Unlink)
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
        "prune" => {
            let blob = repo.find_blob(entry.id())?;
            let content = std::str::from_utf8(blob.content())?;
            if content == "trivial-merge" {
                Ok(Op::Prune)
            } else {
                Err(anyhow!("Invalid prune content"))
            }
        }
        "hook" => {
            let inner = repo.find_tree(entry.id())?;
            let hook_blob =
                repo.find_blob(inner.get_name("0").context("hook: missing hook name")?.id())?;
            let hook_name = std::str::from_utf8(hook_blob.content())?.to_string();
            Ok(Op::Hook(hook_name))
        }
        "author" => {
            let inner = repo.find_tree(entry.id())?;
            let name_blob =
                repo.find_blob(inner.get_name("0").context("author: missing name")?.id())?;
            let email_blob =
                repo.find_blob(inner.get_name("1").context("author: missing email")?.id())?;
            let name = std::str::from_utf8(name_blob.content())?.to_string();
            let email = std::str::from_utf8(email_blob.content())?.to_string();
            Ok(Op::Author(name, email))
        }
        "committer" => {
            let inner = repo.find_tree(entry.id())?;
            let name_blob =
                repo.find_blob(inner.get_name("0").context("committer: missing name")?.id())?;
            let email_blob = repo.find_blob(
                inner
                    .get_name("1")
                    .context("committer: missing email")?
                    .id(),
            )?;
            let name = std::str::from_utf8(name_blob.content())?.to_string();
            let email = std::str::from_utf8(email_blob.content())?.to_string();
            Ok(Op::Committer(name, email))
        }
        "message" => {
            let inner = repo.find_tree(entry.id())?;
            let fmt_blob = repo.find_blob(
                inner
                    .get_name("0")
                    .context("message: missing fmt string")?
                    .id(),
            )?;
            let regex_blob =
                repo.find_blob(inner.get_name("1").context("message: missing regex")?.id())?;
            let fmt = std::str::from_utf8(fmt_blob.content())?.to_string();
            let regex_str = std::str::from_utf8(regex_blob.content())?;
            let regex = regex::Regex::new(regex_str).context("invalid regex")?;
            Ok(Op::Message(fmt, regex))
        }
        "subdir" => {
            let inner = repo.find_tree(entry.id())?;
            let path_blob =
                repo.find_blob(inner.get_name("0").context("subdir: missing path")?.id())?;
            let path = std::str::from_utf8(path_blob.content())?;
            Ok(Op::Subdir(std::path::PathBuf::from(path)))
        }
        "prefix" => {
            let inner = repo.find_tree(entry.id())?;
            let path_blob =
                repo.find_blob(inner.get_name("0").context("prefix: missing path")?.id())?;
            let path = std::str::from_utf8(path_blob.content())?;
            Ok(Op::Prefix(std::path::PathBuf::from(path)))
        }
        "file" => {
            let inner = repo.find_tree(entry.id())?;
            let dest_blob = repo.find_blob(
                inner
                    .get_name("0")
                    .context("file: missing destination path")?
                    .id(),
            )?;
            let source_blob = repo.find_blob(
                inner
                    .get_name("1")
                    .context("file: missing source path")?
                    .id(),
            )?;
            let dest_path_str = std::str::from_utf8(dest_blob.content())?.to_string();
            let source_path_str = std::str::from_utf8(source_blob.content())?.to_string();
            Ok(Op::File(
                std::path::PathBuf::from(dest_path_str),
                std::path::PathBuf::from(source_path_str),
            ))
        }
        #[cfg(feature = "incubating")]
        "embed" => {
            let inner = repo.find_tree(entry.id())?;
            let path_blob =
                repo.find_blob(inner.get_name("0").context("embed: missing path")?.id())?;
            let path = std::str::from_utf8(path_blob.content())?;
            Ok(Op::Embed(std::path::PathBuf::from(path)))
        }
        "pattern" => {
            let inner = repo.find_tree(entry.id())?;
            let pattern_blob = repo.find_blob(
                inner
                    .get_name("0")
                    .context("pattern: missing pattern")?
                    .id(),
            )?;
            let pattern = std::str::from_utf8(pattern_blob.content())?.to_string();
            Ok(Op::Pattern(pattern))
        }
        "workspace" => {
            let inner = repo.find_tree(entry.id())?;
            let path_blob =
                repo.find_blob(inner.get_name("0").context("workspace: missing path")?.id())?;
            let path = std::str::from_utf8(path_blob.content())?;
            Ok(Op::Workspace(std::path::PathBuf::from(path)))
        }
        "stored" => {
            let inner = repo.find_tree(entry.id())?;
            let path_blob =
                repo.find_blob(inner.get_name("0").context("stored: missing path")?.id())?;
            let path = std::str::from_utf8(path_blob.content())?;
            Ok(Op::Stored(std::path::PathBuf::from(path)))
        }
        #[cfg(feature = "incubating")]
        "starlark" => {
            let inner = repo.find_tree(entry.id())?;
            let path_tree = inner.get_name("0").context("starlark: missing path")?;
            let path_blob = repo.find_blob(
                repo.find_tree(path_tree.id())?
                    .get_name("0")
                    .context("starlark: missing path blob")?
                    .id(),
            )?;
            let path = std::str::from_utf8(path_blob.content())?;
            let filter_tree = repo.find_tree(
                inner
                    .get_name("1")
                    .context("starlark: missing filter")?
                    .id(),
            )?;
            let filter = from_tree2(repo, filter_tree.id())?;
            Ok(Op::Starlark(
                std::path::PathBuf::from(path),
                to_filter(filter),
            ))
        }
        "compose" => {
            let compose_tree = repo.find_tree(entry.id())?;
            let mut filters = Vec::new();
            for i in 0..compose_tree.len() {
                let compose_entry = compose_tree.get(i).context("compose: missing entry")?;
                let filter_tree = repo.find_tree(compose_entry.id())?;
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
                        .context("subtract: missing 0")?
                        .id(),
                )?;
                let b_tree = repo.find_tree(
                    subtract_tree
                        .get_name("1")
                        .context("subtract: missing 1")?
                        .id(),
                )?;
                let a = from_tree2(repo, a_tree.id())?;
                let b = from_tree2(repo, b_tree.id())?;
                Ok(Op::Subtract(to_filter(a), to_filter(b)))
            } else {
                Err(anyhow!("subtract: expected 2 entries"))
            }
        }
        "chain" => {
            let chain_tree = repo.find_tree(entry.id())?;
            if !chain_tree.is_empty() {
                let mut filters = vec![];
                for i in 0..chain_tree.len() {
                    let filter_tree = repo.find_tree(
                        chain_tree
                            .get_name(&i.to_string())
                            .with_context(|| format!("chain: missing {}", i))?
                            .id(),
                    )?;
                    let filter = from_tree2(repo, filter_tree.id())?;
                    filters.push(to_filter(filter));
                }
                Ok(Op::Chain(filters))
            } else {
                Err(anyhow!("chain: expected at least 1 entry"))
            }
        }
        "exclude" => {
            let exclude_tree = repo.find_tree(entry.id())?;
            if exclude_tree.len() == 1 {
                let filter_tree = repo.find_tree(
                    exclude_tree
                        .get_name("0")
                        .context("exclude: missing 0")?
                        .id(),
                )?;
                let filter = from_tree2(repo, filter_tree.id())?;
                Ok(Op::Exclude(to_filter(filter)))
            } else {
                Err(anyhow!("exclude: expected 1 entry"))
            }
        }
        "pin" => {
            let pin_tree = repo.find_tree(entry.id())?;
            if pin_tree.len() == 1 {
                let filter_tree =
                    repo.find_tree(pin_tree.get_name("0").context("pin: missing 0")?.id())?;
                let filter = from_tree2(repo, filter_tree.id())?;
                Ok(Op::Pin(to_filter(filter)))
            } else {
                Err(anyhow!("pin: expected 1 entry"))
            }
        }
        "rev" => {
            let rev_tree = repo.find_tree(entry.id())?;
            let mut filters = Vec::new();
            for i in 0..rev_tree.len() {
                let rev_entry = rev_tree
                    .get_name(&i.to_string())
                    .context("rev: missing entry")?;
                let inner_tree = repo.find_tree(rev_entry.id())?;
                let key_blob =
                    repo.find_blob(inner_tree.get_name("o").context("rev: missing key")?.id())?;
                let filter_tree = repo.find_tree(
                    inner_tree
                        .get_name("f")
                        .context("rev: missing filter")?
                        .id(),
                )?;
                let key = std::str::from_utf8(key_blob.content())?;

                // Parse match operator from key
                let (match_op, lazy_ref) = if key == "_" {
                    // Default filter - no SHA needed
                    (RevMatch::Default, LazyRef::Resolved(git2::Oid::zero()))
                } else if let Some(ref_str) = key.strip_prefix("<=") {
                    (RevMatch::AncestorInclusive, LazyRef::parse(ref_str)?)
                } else if let Some(ref_str) = key.strip_prefix('<') {
                    (RevMatch::AncestorStrict, LazyRef::parse(ref_str)?)
                } else if let Some(ref_str) = key.strip_prefix("==") {
                    (RevMatch::Equal, LazyRef::parse(ref_str)?)
                } else {
                    return Err(anyhow!(
                        "rev: invalid key format, must start with '<', '<=', '==', or be '_': {}",
                        key
                    ));
                };

                let filter = from_tree2(repo, filter_tree.id())?;
                filters.push((match_op, lazy_ref, to_filter(filter)));
            }
            Ok(Op::Rev(filters))
        }
        #[cfg(feature = "incubating")]
        "unapply" => {
            let concat_tree = repo.find_tree(entry.id())?;
            let unapply_entry = concat_tree.get(0).context("concat: missing entry")?;
            let inner_tree = repo.find_tree(unapply_entry.id())?;
            let key_blob = repo.find_blob(
                inner_tree
                    .get_name("o")
                    .context("concat: missing key")?
                    .id(),
            )?;
            let filter_tree = repo.find_tree(
                inner_tree
                    .get_name("f")
                    .context("concat: missing filter")?
                    .id(),
            )?;
            let key = std::str::from_utf8(key_blob.content())?;
            let filter = from_tree2(repo, filter_tree.id())?;
            Ok(Op::Unapply(LazyRef::parse(&key)?, to_filter(filter)))
        }
        "squash" => {
            // blob -> Squash(None), tree -> Squash(Some(...))
            if let Some(kind) = entry.kind()
                && kind == git2::ObjectType::Blob
            {
                let _ = repo.find_blob(entry.id())?;
                return Ok(Op::Squash(None));
            }
            let squash_tree = repo.find_tree(entry.id())?;
            let mut filters = std::collections::BTreeMap::new();
            for i in 0..squash_tree.len() {
                let squash_entry = squash_tree.get(i).context("squash: missing entry")?;
                let inner_tree = repo.find_tree(squash_entry.id())?;
                let key_blob = repo.find_blob(
                    inner_tree
                        .get_name("o")
                        .context("squash: missing key")?
                        .id(),
                )?;
                let filter_tree = repo.find_tree(
                    inner_tree
                        .get_name("f")
                        .context("squash: missing filter")?
                        .id(),
                )?;
                let key = std::str::from_utf8(key_blob.content())?;
                let filter = from_tree2(repo, filter_tree.id())?;
                filters.insert(LazyRef::parse(&key)?, to_filter(filter));
            }
            Ok(Op::Squash(Some(filters)))
        }
        "regex_replace" => {
            let regex_replace_tree = repo.find_tree(entry.id())?;
            let mut replacements = Vec::new();
            for i in 0..regex_replace_tree.len() {
                let regex_entry = regex_replace_tree
                    .get(i)
                    .context("regex_replace: missing entry")?;
                let inner_tree = repo.find_tree(regex_entry.id())?;
                let regex_blob = repo.find_blob(
                    inner_tree
                        .get_name("p")
                        .context("regex_replace: missing pattern")?
                        .id(),
                )?;
                let replacement_blob = repo.find_blob(
                    inner_tree
                        .get_name("r")
                        .context("regex_replace: missing replacement")?
                        .id(),
                )?;
                let regex_str = std::str::from_utf8(regex_blob.content())?;
                let replacement = std::str::from_utf8(replacement_blob.content())?.to_string();
                let regex = regex::Regex::new(regex_str)?;
                replacements.push((regex, replacement));
            }
            Ok(Op::RegexReplace(replacements))
        }
        "meta" => {
            let meta_tree = repo.find_tree(entry.id())?;
            let filter_tree = repo.find_tree(
                meta_tree
                    .get_name("0")
                    .context("meta: missing filter tree")?
                    .id(),
            )?;

            // Deserialize metadata map - keys are filenames, values are blob contents
            let mut meta = std::collections::BTreeMap::new();
            for i in 0..meta_tree.len() {
                let meta_entry = meta_tree.get(i).context("meta: missing metadata entry")?;
                let meta_key = meta_entry.name().context("meta: missing metadata key")?;

                // Skip the "0" entry (filter)
                if meta_key == "0" {
                    continue;
                }

                // The entry should be a blob with the value as content
                let value_blob = repo.find_blob(meta_entry.id())?;
                let value = std::str::from_utf8(value_blob.content())?.to_string();
                meta.insert(meta_key.to_string(), value);
            }

            // Deserialize filter
            let filter = from_tree2(repo, filter_tree.id())?;
            Ok(Op::Meta(meta, to_filter(filter)))
        }
        _ => Err(anyhow!("Unknown tree structure")),
    }
}
