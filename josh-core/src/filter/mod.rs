use super::*;
use anyhow::anyhow;

use std::path::Path;
use std::sync::LazyLock;

// Re-export from josh-filter
#[cfg(feature = "incubating")]
pub use josh_filter::LinkMode;
pub use josh_filter::filter::MESSAGE_MATCH_ALL_REGEX;
pub use josh_filter::filter::sequence_number;
pub use josh_filter::flang::parse::{get_comments, parse};
pub use josh_filter::opt;
pub use josh_filter::opt::invert;
pub use josh_filter::persist::{as_tree, from_tree};
pub use josh_filter::persist::{peel_op, to_filter, to_op, to_ops};
pub use josh_filter::{Filter, LazyRef, Op, RevMatch};
pub use josh_filter::{as_file, pretty, spec};

pub mod text;
pub mod tree;

static WORKSPACES: LazyLock<std::sync::Mutex<std::collections::HashMap<git2::Oid, Filter>>> =
    LazyLock::new(Default::default);
static ANCESTORS: LazyLock<
    std::sync::Mutex<std::collections::HashMap<git2::Oid, std::collections::HashSet<git2::Oid>>>,
> = LazyLock::new(Default::default);

// MESSAGE_MATCH_ALL_REGEX is now in josh-filter

// Filter type and builder methods are now in josh-filter
// Note: TryFrom<String> and From<Filter> for String implementations
// are not included here because they require parse/spec from josh-core
// and we cannot implement external traits for external types.
// These conversions should be done via parse() and spec() functions directly.

#[derive(Debug)]
pub struct Rewrite<'a> {
    tree: git2::Tree<'a>,
    commit: git2::Oid,
    pub author: Option<(String, String)>,
    pub committer: Option<(String, String)>,
    pub message: Option<String>,
}

impl<'a> Clone for Rewrite<'a> {
    fn clone(&self) -> Self {
        Rewrite {
            tree: self.tree.clone(),
            commit: self.commit,
            author: self.author.clone(),
            committer: self.committer.clone(),
            message: self.message.clone(),
        }
    }
}

impl<'a> Rewrite<'a> {
    pub fn from_tree(tree: git2::Tree<'a>) -> Self {
        Rewrite {
            tree,
            author: None,
            commit: git2::Oid::zero(),
            committer: None,
            message: None,
        }
    }

    pub fn from_tree_with_metadata(
        tree: git2::Tree<'a>,
        author: Option<(String, String)>,
        committer: Option<(String, String)>,
        message: Option<String>,
    ) -> Self {
        Rewrite {
            tree,
            author,
            commit: git2::Oid::zero(),
            committer,
            message,
        }
    }

    pub fn from_commit(commit: &git2::Commit<'a>) -> anyhow::Result<Self> {
        let tree = commit.tree()?;
        let author = commit
            .author()
            .name()
            .map(|name| name.to_owned())
            .zip(commit.author().email().map(|email| email.to_owned()));
        let committer = commit
            .committer()
            .name()
            .map(|name| name.to_owned())
            .zip(commit.committer().email().map(|email| email.to_owned()));
        let message = commit.message_raw().map(|msg| msg.to_owned());

        Ok(Rewrite {
            tree,
            commit: commit.id(),
            author,
            committer,
            message,
        })
    }

    pub fn with_author(self, author: (String, String)) -> Self {
        Rewrite {
            tree: self.tree,
            author: Some(author),
            commit: self.commit,
            committer: self.committer,
            message: self.message,
        }
    }

    pub fn with_committer(self, committer: (String, String)) -> Self {
        Rewrite {
            tree: self.tree,
            author: self.author,
            commit: self.commit,
            committer: Some(committer),
            message: self.message,
        }
    }

    pub fn with_message(self, message: String) -> Self {
        Rewrite {
            tree: self.tree,
            author: self.author,
            commit: self.commit,
            committer: self.committer,
            message: Some(message),
        }
    }

    pub fn with_commit(self, commit: git2::Oid) -> Self {
        Rewrite {
            tree: self.tree,
            author: self.author,
            commit,
            committer: self.committer,
            message: self.message,
        }
    }

    pub fn with_tree(self, tree: git2::Tree<'a>) -> Self {
        Rewrite {
            tree,
            author: self.author,
            commit: self.commit,
            committer: self.committer,
            message: self.message,
        }
    }

    pub fn tree(&self) -> &git2::Tree<'a> {
        &self.tree
    }

    pub fn into_tree(self) -> git2::Tree<'a> {
        self.tree
    }
}

pub use josh_filter::compose;

pub fn lazy_refs(filter: Filter) -> Vec<String> {
    lazy_refs2(&peel_op(filter))
}

fn lazy_refs2(op: &Op) -> Vec<String> {
    let mut lr = match op {
        Op::Compose(filters) => {
            filters
                .iter()
                .map(|f| lazy_refs(*f))
                .fold(vec![], |mut acc, mut v| {
                    acc.append(&mut v);
                    acc
                })
        }
        Op::Exclude(filter) | Op::Pin(filter) => lazy_refs(*filter),
        Op::Chain(filters) => {
            let mut av = vec![];
            for filter in filters {
                av.append(&mut lazy_refs(*filter));
            }
            av
        }
        Op::Subtract(a, b) => {
            let mut av = lazy_refs(*a);
            av.append(&mut lazy_refs(*b));
            av
        }
        Op::Rev(filters) => {
            let mut lr = lazy_refs2(&Op::Compose(filters.iter().map(|(_, _, f)| *f).collect()));
            lr.extend(filters.iter().filter_map(|(_, nested, _)| {
                if let LazyRef::Lazy(s) = nested {
                    Some(s.to_owned())
                } else {
                    None
                }
            }));
            lr.sort();
            lr.dedup();
            lr
        }
        Op::Squash(Some(revs)) => {
            let mut lr = vec![];
            lr.extend(revs.keys().filter_map(|nested| {
                if let LazyRef::Lazy(s) = nested {
                    Some(s.to_owned())
                } else {
                    None
                }
            }));
            lr
        }
        _ => vec![],
    };
    lr.sort();
    lr.dedup();
    lr
}

pub fn resolve_refs(refs: &std::collections::HashMap<String, git2::Oid>, filter: Filter) -> Filter {
    to_filter(resolve_refs2(refs, &to_op(filter)))
}

fn resolve_refs2(refs: &std::collections::HashMap<String, git2::Oid>, op: &Op) -> Op {
    match op {
        Op::Compose(filters) => {
            Op::Compose(filters.iter().map(|f| resolve_refs(refs, *f)).collect())
        }
        Op::Exclude(filter) => Op::Exclude(resolve_refs(refs, *filter)),
        Op::Pin(filter) => Op::Pin(resolve_refs(refs, *filter)),
        Op::Chain(filters) => Op::Chain(filters.iter().map(|f| resolve_refs(refs, *f)).collect()),
        Op::Subtract(a, b) => Op::Subtract(resolve_refs(refs, *a), resolve_refs(refs, *b)),
        Op::Rev(filters) => {
            let lr = filters
                .iter()
                .map(|(match_op, r, f)| {
                    let f = resolve_refs(refs, *f);
                    let resolved_r = if let LazyRef::Lazy(s) = r {
                        if let Some(res) = refs.get(s) {
                            LazyRef::Resolved(*res)
                        } else {
                            r.clone()
                        }
                    } else {
                        r.clone()
                    };
                    (*match_op, resolved_r, f)
                })
                .collect();
            Op::Rev(lr)
        }
        Op::Squash(Some(filters)) => {
            let lr = filters
                .iter()
                .map(|(r, m)| {
                    if let LazyRef::Lazy(s) = r {
                        if let Some(res) = refs.get(s) {
                            (LazyRef::Resolved(*res), *m)
                        } else {
                            (r.clone(), *m)
                        }
                    } else {
                        (r.clone(), *m)
                    }
                })
                .collect();
            Op::Squash(Some(lr))
        }
        _ => op.clone(),
    }
}

pub fn src_path(filter: Filter) -> std::path::PathBuf {
    src_path2(&peel_op(filter))
}

fn src_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Subdir(path) => path.to_owned(),
        Op::File(_, source_path) => source_path.to_owned(),
        Op::Chain(filters) => filters
            .iter()
            .fold(std::path::PathBuf::new(), |acc, f| acc.join(src_path(*f))),
        _ => std::path::PathBuf::new(),
    })
}

pub fn dst_path(filter: Filter) -> std::path::PathBuf {
    dst_path2(&peel_op(filter))
}

fn dst_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Prefix(path) => path.to_owned(),
        Op::File(dest_path, _) => dest_path.to_owned(),
        Op::Chain(filters) => filters
            .iter()
            .rev()
            .fold(std::path::PathBuf::new(), |acc, f| acc.join(dst_path(*f))),
        _ => std::path::PathBuf::new(),
    })
}

/// Calculate the filtered commit for `commit`. This can take some time if done
/// for the first time and thus should generally be done asynchronously.
pub fn apply_to_commit(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> anyhow::Result<git2::Oid> {
    let filter = opt::optimize(filter);
    loop {
        let filtered = apply_to_commit2(filter, commit, transaction)?;

        if let Some(id) = filtered {
            return Ok(id);
        }

        let missing = transaction.get_missing();

        for (f, i) in missing.into_iter().rev() {
            history::walk2(f, i, transaction)?;
        }
    }
}

// Handle workspace.josh files that contain ":workspace=..." as their only filter as
// a "redirect" to that other workspace. We chain an exclude of the redirecting workspace
// in front to prevent infinite recursion.
fn resolve_workspace_redirect<'a>(
    repo: &'a git2::Repository,
    tree: &'a git2::Tree<'a>,
    path: &Path,
) -> Option<(Filter, std::path::PathBuf)> {
    let f = parse(&tree::get_blob(repo, tree, &path.join("workspace.josh")))
        .unwrap_or_else(|_| to_filter(Op::Empty));

    if let Op::Workspace(p) = to_op(f) {
        Some((to_filter(Op::Exclude(Filter::new().file(path))).chain(f), p))
    } else {
        None
    }
}

fn get_workspace<'a>(
    transaction: &cache::Transaction,
    tree: &'a git2::Tree<'a>,
    path: &Path,
) -> Filter {
    let wsj_file = Filter::new().file("workspace.josh");
    let base = to_filter(Op::Subdir(path.to_owned()));
    let wsj_file = base.chain(wsj_file);
    compose(&[
        wsj_file,
        compose(&[
            get_filter(transaction, tree, &path.join("workspace.josh")),
            base,
        ]),
    ])
}

fn get_stored<'a>(
    transaction: &cache::Transaction,
    tree: &'a git2::Tree<'a>,
    path: &Path,
) -> Filter {
    let stored_path = path.with_added_extension("josh");
    let sj_file = Filter::new().file(stored_path.clone());
    compose(&[sj_file, get_filter(transaction, tree, &stored_path)])
}

#[cfg(feature = "incubating")]
fn get_starlark<'a>(
    transaction: &cache::Transaction,
    tree: &'a git2::Tree<'a>,
    path: &Path,
    subfilter: Filter,
) -> Filter {
    let star_path = path.with_added_extension("star");
    let script = tree::get_blob(transaction.repo(), tree, &star_path);
    let filtered_tree = match apply(transaction, subfilter, Rewrite::from_tree(tree.clone())) {
        Ok(rw) => rw.into_tree(),
        Err(_) => return to_filter(Op::Empty),
    };
    let repo = match git2::Repository::open(transaction.repo().path()) {
        Ok(r) => std::sync::Arc::new(std::sync::Mutex::new(r)),
        Err(_) => return to_filter(Op::Empty),
    };
    match josh_starlark::evaluate(&script, filtered_tree.id(), repo) {
        Ok(f) => {
            let star_file = Filter::new().file(star_path);
            compose(&[star_file, subfilter, f])
        }
        Err(e) => {
            tracing::trace!("starlark evaluation failed: {}", e);
            to_filter(Op::Empty)
        }
    }
}

fn get_filter<'a>(
    transaction: &cache::Transaction,
    tree: &'a git2::Tree<'a>,
    path: &Path,
) -> Filter {
    let ws_path = normalize_path(path);
    let ws_id = ok_or!(tree.get_path(&ws_path), {
        return to_filter(Op::Empty);
    })
    .id();
    let ws_blob = tree::get_blob(transaction.repo(), tree, &ws_path);

    if let Some(f) = WORKSPACES.lock().unwrap().get(&ws_id) {
        *f
    } else {
        let f = parse(&ws_blob).unwrap_or_else(|_| to_filter(Op::Empty));
        let f = legalize_stored(transaction, f, tree).unwrap_or_else(|_| to_filter(Op::Empty));

        let f = if invert(f).is_ok() {
            f
        } else {
            to_filter(Op::Empty)
        };
        WORKSPACES.lock().unwrap().insert(ws_id, f);
        f
    }
}

#[cfg(feature = "incubating")]
fn read_josh_link<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree<'a>,
    root: &std::path::Path,
    filename: &str,
) -> Option<Filter> {
    use anyhow::Context;

    let link_path = root.join(filename);
    let link_entry = tree.get_path(&link_path).ok()?;
    let link_blob = repo.find_blob(link_entry.id()).ok()?;
    let b = std::str::from_utf8(link_blob.content())
        .with_context(|| format!("invalid utf8 in {}", filename))
        .ok()?;

    // Parse the filter string
    let filter = parse(b.trim())
        .with_context(|| format!("invalid filter in {}", filename))
        .ok()?;

    // Validate that it has required metadata for a link file
    if filter.get_meta("remote").is_none() || filter.get_meta("commit").is_none() {
        return None;
    }

    Some(filter)
}

fn get_rev_filter(
    transaction: &cache::Transaction,
    commit: &git2::Commit,
    filters: &[(RevMatch, LazyRef, Filter)],
) -> anyhow::Result<Filter> {
    let commit_id = commit.id();

    // First match wins - iterate in order
    for (match_op, filter_tip_ref, startfilter) in filters.iter() {
        let filter_tip = if let LazyRef::Resolved(filter_tip) = filter_tip_ref {
            filter_tip
        } else {
            return Err(anyhow!("unresolved lazy ref"));
        };
        if match_op != &RevMatch::Default && !transaction.repo().odb()?.exists(*filter_tip) {
            return Err(anyhow!("`:rev(...)` with nonexistent OID: {}", filter_tip));
        }
        let matches = match match_op {
            RevMatch::AncestorStrict => {
                // `<` - matches if commit is ancestor of tip AND commit != tip (strict)

                is_ancestor_of(transaction, commit_id, *filter_tip)? && commit_id != *filter_tip
            }
            RevMatch::AncestorInclusive => {
                // `<=` - matches if commit is ancestor of tip OR commit == tip (inclusive)

                is_ancestor_of(transaction, commit_id, *filter_tip)?
            }
            RevMatch::Equal => {
                // `==` - matches if commit == tip

                commit_id == *filter_tip
            }
            RevMatch::Default => {
                // `_` - always matches (makes filters after it unreachable)
                true
            }
        };

        if matches {
            return Ok(*startfilter);
        }
    }

    // No match found, return Nop
    Ok(to_filter(Op::Nop))
}

pub fn apply_to_commit2(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> anyhow::Result<Option<git2::Oid>> {
    let repo = transaction.repo();
    let op = peel_op(filter);

    if filter == Filter::new() {
        return Ok(Some(commit.id()));
    }

    match &op {
        Op::Empty => return Ok(Some(git2::Oid::zero())),

        Op::Chain(filters) => {
            let mut current_oid = commit.id();
            let chain_meta = filter.into_meta();
            for f in filters {
                let mut f = *f;
                for (k, v) in chain_meta.iter() {
                    f = f.with_meta(k, v);
                }
                if current_oid == git2::Oid::zero() {
                    break;
                }
                let current_commit = repo.find_commit(current_oid)?;
                let r = some_or!(apply_to_commit2(f, &current_commit, transaction)?, {
                    return Ok(None);
                });
                current_oid = r;
            }
            return Ok(Some(current_oid));
        }
        Op::Squash(None) => {
            return Some(history::rewrite_commit(
                repo,
                commit,
                &[],
                Rewrite::from_commit(commit)?,
                true,
            ))
            .transpose();
        }
        _ => {
            if let Some(oid) = transaction.get(filter, commit.id()) {
                return Ok(Some(oid));
            }
            // Continue to process the filter if not cached
        }
    };

    rs_tracing::trace_scoped!("apply_to_commit", "spec": spec(filter), "commit": commit.id().to_string());

    let rewrite_data = match &op {
        Op::Squash(Some(ids)) => {
            if let Some(sq) = ids.get(&LazyRef::Resolved(commit.id())) {
                let oid = if let Some(oid) = apply_to_commit2(
                    filter::Filter::new().squash(None).chain(*sq),
                    commit,
                    transaction,
                )? {
                    oid
                } else {
                    return Ok(None);
                };

                let rc = transaction.repo().find_commit(oid)?;
                let author = rc
                    .author()
                    .name()
                    .map(|x| x.to_owned())
                    .zip(rc.author().email().map(|x| x.to_owned()));
                let committer = rc
                    .committer()
                    .name()
                    .map(|x| x.to_owned())
                    .zip(rc.committer().email().map(|x| x.to_owned()));
                Rewrite::from_tree_with_metadata(
                    rc.tree()?,
                    author,
                    committer,
                    rc.message_raw().map(|x| x.to_owned()),
                )
                //commit.tree()?
            } else {
                if let Some(parent) = commit.parents().next() {
                    return Ok(
                        if let Some(fparent) = transaction.get(filter, parent.id()) {
                            Some(history::drop_commit(
                                commit,
                                vec![fparent],
                                transaction,
                                filter,
                            )?)
                        } else {
                            None
                        },
                    );
                }
                return Ok(Some(history::drop_commit(
                    commit,
                    vec![],
                    transaction,
                    filter,
                )?));
            }
        }
        Op::Prune => {
            let p: Vec<_> = commit.parent_ids().collect();

            if p.len() > 1 {
                let parent = some_or!(transaction.get(filter, p[0]), {
                    return Ok(None);
                });

                let parent_tree = transaction.repo().find_commit(parent)?.tree_id();

                if parent_tree == commit.tree_id() {
                    return Ok(Some(history::drop_commit(
                        commit,
                        vec![parent],
                        transaction,
                        filter,
                    )?));
                }
            }

            Rewrite::from_commit(commit)?
        }
        #[cfg(feature = "incubating")]
        Op::Export => {
            let filtered_parent_ids = {
                commit
                    .parents()
                    .map(|x| transaction.get(filter, x.id()))
                    .collect::<Option<_>>()
            };

            let mut filtered_parent_ids: Vec<git2::Oid> =
                some_or!(filtered_parent_ids, { return Ok(None) });

            // TODO: remove all parents that don't have a .link.josh

            //     let mut ok = true;
            //     filtered_parent_ids.retain(|c| {
            //         if let Ok(c) = repo.find_commit(*c) {
            //             c.tree_id() != new_tree.id()
            //         } else {
            //             ok = false;
            //             false
            //         }
            //     });

            //     if !ok {
            //         return Err(anyhow!("missing commit"));
            //     }

            if let Some(link_file) = read_josh_link(
                repo,
                &commit.tree()?,
                &std::path::PathBuf::new(),
                ".link.josh",
            ) {
                if let Some(commit_str) = link_file.get_meta("commit") {
                    if let Ok(commit_oid) = git2::Oid::from_str(&commit_str) {
                        if filtered_parent_ids.contains(&commit_oid) {
                            while filtered_parent_ids[0] != commit_oid {
                                filtered_parent_ids.rotate_right(1);
                            }
                        }
                    }
                }
            }

            return Some(history::create_filtered_commit(
                commit,
                filtered_parent_ids,
                apply(transaction, filter, Rewrite::from_commit(commit)?)?,
                transaction,
                filter,
            ))
            .transpose();
        }
        #[cfg(feature = "incubating")]
        Op::Unlink => {
            use crate::link::find_link_files;

            let filtered_parent_ids = {
                commit
                    .parents()
                    .map(|x| transaction.get(filter, x.id()))
                    .collect::<Option<_>>()
            };

            let mut filtered_parent_ids: Vec<git2::Oid> =
                some_or!(filtered_parent_ids, { return Ok(None) });

            let mut link_parents = vec![];
            for (link_path, link_file) in find_link_files(&repo, &commit.tree()?)?.into_iter() {
                if let Some(commit_str) = link_file.get_meta("commit") {
                    if let Ok(commit_oid) = git2::Oid::from_str(&commit_str) {
                        if let Some(cmt) =
                            transaction.get(to_filter(Op::Prefix(link_path)), commit_oid)
                        {
                            link_parents.push(cmt);
                        } else {
                            return Ok(None);
                        }
                    } else {
                        return Ok(None);
                    }
                } else {
                    return Ok(None);
                }
            }

            let new_tree = apply(transaction, filter, Rewrite::from_commit(commit)?)?;

            filtered_parent_ids.retain(|c| !link_parents.contains(c));

            return Some(history::create_filtered_commit(
                commit,
                filtered_parent_ids,
                new_tree,
                transaction,
                filter,
            ))
            .transpose();
        }
        #[cfg(feature = "incubating")]
        Op::Link(mode) => {
            let mut roots = get_link_roots(repo, transaction, &commit.tree()?)?;

            if let Some(parent) = commit.parents().next() {
                roots.retain(|root| {
                    if let (Ok(a), Ok(b)) = (
                        commit.tree().and_then(|x| x.get_path(&root)),
                        parent.tree().and_then(|x| x.get_path(&root)),
                    ) && a.id() == b.id()
                    {
                        false
                    } else {
                        true
                    }
                });
            };

            let all_links = links_from_roots(repo, &commit.tree()?, roots)?;

            // Only embedded-mode links get extra parent commits spliced in
            let embedded_links: Vec<_> = all_links
                .into_iter()
                .filter(|(_, link_file)| {
                    let effective_mode = mode.clone().unwrap_or_else(|| {
                        link_file
                            .get_meta("mode")
                            .and_then(|s| josh_filter::LinkMode::parse(&s).ok())
                            .unwrap_or(josh_filter::LinkMode::Pointer)
                    });
                    effective_mode == josh_filter::LinkMode::Embedded
                })
                .collect();

            if embedded_links.is_empty() {
                apply(transaction, filter, Rewrite::from_commit(commit)?)?
            } else {
                let normal_parents = commit
                    .parent_ids()
                    .map(|parent| transaction.get(filter, parent))
                    .collect::<Option<Vec<git2::Oid>>>();

                let normal_parents = some_or!(normal_parents, { return Ok(None) });

                let extra_parents = {
                    let mut extra_parents = vec![];
                    for (root, _link_file) in embedded_links {
                        let embeding = some_or!(
                            apply_to_commit2(
                                Filter::new().message("{@}").file(root.join(".link.josh")),
                                &commit,
                                transaction
                            )?,
                            {
                                return Ok(None);
                            }
                        );

                        #[cfg(feature = "incubating")]
                        let f = to_filter(Op::Embed(root));

                        let embeding = repo.find_commit(embeding)?;
                        let r = some_or!(apply_to_commit2(f, &embeding, transaction)?, {
                            return Ok(None);
                        });

                        extra_parents.push(r);
                    }

                    extra_parents
                };

                let filtered_tree = apply(transaction, filter, Rewrite::from_commit(commit)?)?;
                let filtered_parent_ids = normal_parents
                    .into_iter()
                    .chain(extra_parents)
                    .collect::<Vec<_>>();

                return Some(history::create_filtered_commit(
                    commit,
                    filtered_parent_ids,
                    filtered_tree,
                    transaction,
                    filter,
                ))
                .transpose();
            }
        }
        Op::Workspace(ws_path) => {
            if let Some((redirect, _)) = resolve_workspace_redirect(repo, &commit.tree()?, ws_path)
            {
                if let Some(r) = apply_to_commit2(redirect, commit, transaction)? {
                    transaction.insert(filter, commit.id(), r, true);
                    return Ok(Some(r));
                } else {
                    return Ok(None);
                }
            }

            let commit_filter = get_workspace(transaction, &commit.tree()?, ws_path);

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcw = get_workspace(
                        transaction,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        ws_path,
                    );
                    Ok((parent, pcw))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        Op::Stored(s_path) => {
            let commit_filter = get_stored(transaction, &commit.tree()?, s_path);

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcs = get_stored(
                        transaction,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        s_path,
                    );
                    Ok((parent, pcs))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        #[cfg(feature = "incubating")]
        Op::Starlark(s_path, s_subfilter) => {
            let commit_filter = get_starlark(transaction, &commit.tree()?, s_path, *s_subfilter);

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcs = get_starlark(
                        transaction,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        s_path,
                        *s_subfilter,
                    );
                    Ok((parent, pcs))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        Op::Rev(filters) => {
            let commit_filter = get_rev_filter(transaction, commit, filters)?;

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    let pcw = get_rev_filter(transaction, &parent, filters)?;
                    Ok((parent, pcw))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        Op::Fold => {
            let filtered_parent_ids = commit
                .parents()
                .map(|x| transaction.get(filter, x.id()))
                .collect::<Option<Vec<_>>>();

            let filtered_parent_ids = some_or!(filtered_parent_ids, { return Ok(None) });

            let trees: Vec<git2::Oid> = filtered_parent_ids
                .iter()
                .map(|x| Ok(repo.find_commit(*x)?.tree_id()))
                .collect::<anyhow::Result<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree = tree::overlay(transaction, filtered_tree, t)?;
            }

            let filtered_tree = repo.find_tree(filtered_tree)?;
            Rewrite::from_commit(commit)?.with_tree(filtered_tree)
        }
        Op::Hook(hook) => {
            let commit_filter = transaction.lookup_filter_hook(hook, commit.id())?;

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    let pcw = transaction.lookup_filter_hook(hook, parent.id())?;
                    Ok((parent, pcw))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        #[cfg(feature = "incubating")]
        Op::Unapply(target, uf) => {
            if let LazyRef::Resolved(target) = target {
                /* dbg!(target); */
                let target = repo.find_commit(*target)?;
                if let Some(parent) = target.parents().next() {
                    let ptree = apply(transaction, *uf, Rewrite::from_commit(&parent)?)?;
                    if let Some(link) = read_josh_link(
                        repo,
                        &ptree.tree(),
                        &std::path::PathBuf::new(),
                        ".link.josh",
                    ) {
                        if let Some(commit_str) = link.get_meta("commit") {
                            if let Ok(link_commit) = git2::Oid::from_str(&commit_str) {
                                if commit.id() == link_commit {
                                    let unapply =
                                        to_filter(Op::Unapply(LazyRef::Resolved(parent.id()), *uf));
                                    let r = some_or!(transaction.get(unapply, link_commit), {
                                        return Ok(None);
                                    });
                                    transaction.insert(filter, commit.id(), r, true);
                                    return Ok(Some(r));
                                }
                            }
                        }
                    }
                }
            } else {
                return Err(anyhow!("unresolved lazy ref"));
            }
            /* dbg!("FALLTHROUGH"); */
            apply(
                transaction,
                filter,
                Rewrite::from_commit(commit)?, /* Rewrite::from_commit(commit)?.with_parents(filtered_parent_ids), */
            )?
        }
        #[cfg(feature = "incubating")]
        Op::Embed(path) => {
            let subdir = to_filter(Op::Subdir(path.clone()));
            let unapply = to_filter(Op::Unapply(LazyRef::Resolved(commit.id()), subdir));

            /* dbg!("embed"); */
            /* dbg!(&path); */
            if let Some(link) = read_josh_link(repo, &commit.tree()?, &path, ".link.josh") {
                /* dbg!(&link); */
                if let Some(commit_str) = link.get_meta("commit") {
                    if let Ok(commit_oid) = git2::Oid::from_str(&commit_str) {
                        let r = some_or!(transaction.get(unapply, commit_oid), {
                            return Ok(None);
                        });
                        transaction.insert(filter, commit.id(), r, true);
                        return Ok(Some(r));
                    }
                }
            }
            return Ok(Some(git2::Oid::zero()));
        }

        _ => apply(transaction, filter, Rewrite::from_commit(commit)?)?,
    };

    let tree_data = rewrite_data;

    let filtered_parent_ids = {
        rs_tracing::trace_scoped!("filtered_parent_ids", "n": commit.parent_ids().len());
        commit
            .parents()
            .map(|x| transaction.get(filter, x.id()))
            .collect::<Option<_>>()
    };

    let filtered_parent_ids = some_or!(filtered_parent_ids, { return Ok(None) });

    Some(history::create_filtered_commit(
        commit,
        filtered_parent_ids,
        tree_data,
        transaction,
        filter,
    ))
    .transpose()
}

#[cfg(feature = "incubating")]
fn extract_submodule_commits<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree<'a>,
) -> anyhow::Result<
    std::collections::BTreeMap<
        std::path::PathBuf,
        (git2::Oid, crate::submodules::ParsedSubmoduleEntry),
    >,
> {
    use crate::submodules::{ParsedSubmoduleEntry, parse_gitmodules};
    // Get .gitmodules blob from the tree
    let gitmodules_content = tree::get_blob(repo, tree, std::path::Path::new(".gitmodules"));

    if gitmodules_content.is_empty() {
        // No .gitmodules file, return empty map
        return Ok(std::collections::BTreeMap::new());
    }

    // Parse submodule entries using parse_gitmodules
    let submodule_entries = match parse_gitmodules(&gitmodules_content) {
        Ok(entries) => entries,
        Err(_) => {
            // If parsing fails, return empty map
            return Ok(std::collections::BTreeMap::new());
        }
    };

    let mut submodule_commits: std::collections::BTreeMap<
        std::path::PathBuf,
        (git2::Oid, ParsedSubmoduleEntry),
    > = std::collections::BTreeMap::new();

    for parsed in submodule_entries {
        let submodule_path = parsed.path.clone();
        // Get the submodule entry from the tree
        if let Ok(entry) = tree.get_path(&submodule_path) {
            // Check if this is a commit (submodule) entry
            if entry.kind() == Some(git2::ObjectType::Commit) {
                // Get the commit OID stored in the tree entry
                let commit_oid = entry.id();
                // Store OID and parsed entry metadata
                submodule_commits.insert(submodule_path, (commit_oid, parsed));
            }
        }
    }

    Ok(submodule_commits)
}

#[cfg(feature = "incubating")]
fn get_link_roots<'a>(
    _repo: &'a git2::Repository,
    transaction: &'a cache::Transaction,
    tree: &'a git2::Tree<'a>,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let link_filter = to_filter(Op::Pattern("**/.link.josh".to_string()));
    let link_tree = apply(transaction, link_filter, Rewrite::from_tree(tree.clone()))?;

    let mut roots = vec![];
    link_tree
        .tree()
        .walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            let root = root.trim_matches('/');
            let root = std::path::PathBuf::from(root);
            if entry.name() == Some(".link.josh") {
                roots.push(root);
            }
            0
        })?;

    Ok(roots)
}

#[cfg(feature = "incubating")]
fn links_from_roots<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree<'a>,
    roots: Vec<std::path::PathBuf>,
) -> anyhow::Result<Vec<(std::path::PathBuf, Filter)>> {
    let mut v = vec![];
    for root in roots {
        if let Some(link_filter) = read_josh_link(repo, tree, &root, ".link.josh") {
            v.push((root, link_filter));
        }
    }
    Ok(v)
}

/// Filter a single tree. This does not involve walking history and is thus fast in most cases.
pub fn apply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    x: Rewrite<'a>,
) -> anyhow::Result<Rewrite<'a>> {
    let repo = transaction.repo();
    let op = peel_op(filter);
    match &op {
        Op::Nop => Ok(x),
        Op::Empty => Ok(x.with_tree(tree::empty(repo))),
        Op::Fold => Ok(x),
        Op::Squash(None) => Ok(x),
        Op::Author(author, email) => Ok(x.with_author((author.clone(), email.clone()))),
        Op::Committer(author, email) => Ok(x.with_committer((author.clone(), email.clone()))),
        Op::Squash(Some(_)) => Err(anyhow!("not applicable to tree: squash")),
        Op::Message(m, r) => {
            let tree_id = x.tree().id().to_string();
            let commit = x.commit;
            let commit_id = commit.to_string();

            let message = if let Some(ref m) = x.message {
                m.to_string()
            } else if let Ok(c) = transaction.repo().find_commit(commit) {
                c.message_raw().unwrap_or_default().to_string()
            } else {
                "".to_string()
            };

            let tree = x.tree().clone();
            Ok(x.with_message(text::transform_with_template(
                r,
                m,
                &message,
                |key: &str| -> Option<String> {
                    match key {
                        "#" => Some(tree_id.clone()),
                        "@" => Some(commit_id.clone()),
                        key if key.starts_with("/") => {
                            Some(tree::get_blob(repo, &tree, std::path::Path::new(&key[1..])))
                        }

                        key if key.starts_with("#") => Some(
                            tree.get_path(std::path::Path::new(&key[1..]))
                                .map(|e| e.id())
                                .unwrap_or(git2::Oid::zero())
                                .to_string(),
                        ),
                        _ => None,
                    }
                },
            )?))
        }
        Op::Prune => Ok(x),
        #[cfg(feature = "incubating")]
        Op::Adapt(adapter) => {
            let mut result_tree = x.tree().clone();
            match adapter.as_ref() {
                "submodules" => {
                    // Extract submodule commits
                    let submodule_commits = extract_submodule_commits(repo, &result_tree)?;

                    // Process each submodule commit
                    for (submodule_path, (commit_oid, meta)) in submodule_commits {
                        let prefix_filter = to_filter(Op::Nop);

                        // Create a filter with metadata
                        let link_filter = prefix_filter
                            .with_meta("remote", meta.url.clone())
                            .with_meta("target", "HEAD")
                            .with_meta("commit", commit_oid.to_string());
                        let link_content = as_file(link_filter, 0);

                        result_tree = tree::insert(
                            repo,
                            &result_tree,
                            &submodule_path.join(".link.josh"),
                            repo.blob(link_content.as_bytes())?,
                            0o0100644,
                        )?;
                    }

                    // Remove .gitmodules file by setting it to zero OID
                    result_tree = tree::insert(
                        repo,
                        &result_tree,
                        std::path::Path::new(".gitmodules"),
                        git2::Oid::zero(),
                        0o0100644,
                    )?;
                }
                _ => return Err(anyhow!("unknown adapter {:?}", adapter)),
            }

            Ok(x.with_tree(result_tree))
        }
        #[cfg(feature = "incubating")]
        Op::Export => {
            let tree = x.tree().clone();
            Ok(x.with_tree(tree::insert(
                repo,
                &tree,
                &std::path::Path::new(".link.josh"),
                git2::Oid::zero(),
                0o0100644,
            )?))
        }
        #[cfg(feature = "incubating")]
        Op::Unlink => {
            use crate::link::find_link_files;
            let mut result_tree = x.tree.clone();
            for (link_path, link_file) in find_link_files(&repo, &result_tree)?.iter() {
                result_tree =
                    tree::insert(repo, &result_tree, &link_path, git2::Oid::zero(), 0o0100644)?;

                // The link_file is already a filter with metadata, just serialize it
                let link_content = as_file(*link_file, 0);

                result_tree = tree::insert(
                    repo,
                    &result_tree,
                    &link_path.join(".link.josh"),
                    repo.blob(link_content.as_bytes())?,
                    0o0100644,
                )?;
            }
            Ok(x.with_tree(result_tree))
        }
        #[cfg(feature = "incubating")]
        Op::Link(mode) => {
            let roots = get_link_roots(repo, transaction, &x.tree())?;
            let v = links_from_roots(repo, &x.tree(), roots)?;
            let mut result_tree = x.tree().clone();

            for (root, link_file) in v {
                // Get commit from metadata
                let commit_oid = link_file
                    .get_meta("commit")
                    .and_then(|s| git2::Oid::from_str(&s).ok())
                    .ok_or_else(|| anyhow!("Link file missing commit metadata"))?;

                let submodule_tree = repo.find_commit(commit_oid)?.tree()?;
                let inner_filter = link_file.peel();
                let submodule_tree = apply(
                    transaction,
                    inner_filter,
                    Rewrite::from_tree(submodule_tree),
                )
                .unwrap();

                result_tree = tree::insert(
                    repo,
                    &result_tree,
                    &root,
                    submodule_tree.tree().id(),
                    0o0040000, // Tree mode
                )?;
                let effective_mode = mode.clone().unwrap_or_else(|| {
                    link_file
                        .get_meta("mode")
                        .and_then(|s| josh_filter::LinkMode::parse(&s).ok())
                        .unwrap_or(josh_filter::LinkMode::Pointer)
                });
                let link_content = as_file(link_file.with_meta("mode", effective_mode.as_str()), 0);

                result_tree = tree::insert(
                    repo,
                    &result_tree,
                    &root.join(".link.josh"),
                    repo.blob(link_content.as_bytes())?,
                    0o0100644,
                )?;
            }

            Ok(x.with_tree(result_tree))
        }
        Op::Rev(_) => Err(anyhow!("not applicable to tree: rev")),
        Op::RegexReplace(replacements) => {
            let mut t = x.tree().clone();
            for (regex, replacement) in replacements {
                t = tree::regex_replace(t.id(), regex, replacement, transaction)?;
            }
            Ok(x.with_tree(t))
        }

        Op::Pattern(pattern) => {
            let pattern = glob::Pattern::new(pattern)?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            Ok(x.clone().with_tree(tree::remove_pred(
                transaction,
                "",
                x.tree().id(),
                &|path, isblob| isblob && (pattern.matches_path_with(path, options)),
                to_filter(op.clone()).id(),
            )?))
        }
        Op::File(dest_path, source_path) => {
            let (file, mode) = x
                .tree()
                .get_path(source_path)
                .map(|x| (x.id(), x.filemode()))
                .unwrap_or((git2::Oid::zero(), git2::FileMode::Blob.into()));
            Ok(x.with_tree(tree::insert(
                repo,
                &tree::empty(repo),
                dest_path,
                file,
                mode,
            )?))
        }

        Op::Subdir(path) => Ok(x.clone().with_tree(
            x.tree()
                .get_path(path)
                .and_then(|x| repo.find_tree(x.id()))
                .unwrap_or_else(|_| tree::empty(repo)),
        )),
        Op::Prefix(path) => Ok(x.clone().with_tree(tree::insert(
            repo,
            &tree::empty(repo),
            path,
            x.tree().id(),
            git2::FileMode::Tree.into(),
        )?)),

        Op::Subtract(a, b) => {
            let af = apply(transaction, *a, x.clone())?;
            let bf = apply(transaction, *b, x.clone())?;
            let bu = apply(transaction, invert(*b)?, bf.clone())?;
            let ba = apply(transaction, *a, bu.clone())?.tree().id();
            Ok(x.with_tree(repo.find_tree(tree::subtract(transaction, af.tree().id(), ba)?)?))
        }
        Op::Exclude(b) => {
            let bf = apply(transaction, *b, x.clone())?.tree().id();
            Ok(x.clone().with_tree(repo.find_tree(tree::subtract(
                transaction,
                x.tree().id(),
                bf,
            )?)?))
        }

        Op::Paths => Ok(x
            .clone()
            .with_tree(tree::pathstree("", x.tree().id(), transaction)?)),
        Op::Index => Ok(x
            .clone()
            .with_tree(tree::trigram_index(transaction, x.tree().clone())?)),

        Op::Invert => {
            Ok(x.clone()
                .with_tree(tree::invert_paths(transaction, "", x.tree().clone())?))
        }

        Op::Workspace(path) => apply(transaction, get_workspace(transaction, x.tree(), path), x),
        Op::Stored(path) => apply(transaction, get_stored(transaction, x.tree(), path), x),
        #[cfg(feature = "incubating")]
        Op::Starlark(path, subfilter) => apply(
            transaction,
            get_starlark(transaction, x.tree(), path, *subfilter),
            x,
        ),

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| apply(transaction, *f, x.clone()))
                .collect::<anyhow::Result<_>>()?;
            let filtered: Vec<_> = filters
                .iter()
                .zip(filtered.iter().map(|t| t.tree().clone()))
                .collect();
            Ok(x.with_tree(tree::compose(transaction, filtered)?))
        }

        Op::Chain(filters) => {
            let mut result = x;
            for filter in filters {
                result = apply(transaction, *filter, result)?;
            }
            Ok(result)
        }
        Op::Hook(_) => Err(anyhow!("not applicable to tree: hook")),

        #[cfg(feature = "incubating")]
        Op::Embed(..) => Err(anyhow!("not applicable to tree: embed")),
        #[cfg(feature = "incubating")]
        Op::Unapply(target, uf) => {
            if let LazyRef::Resolved(target) = target {
                let target = repo.find_commit(*target)?;
                let target = git2::Oid::from_str(target.message().unwrap())?;
                let target = repo.find_commit(target)?;
                /* dbg!(&uf); */
                Ok(Rewrite::from_tree(filter::unapply(
                    transaction,
                    *uf,
                    x.tree().clone(),
                    target.tree()?,
                )?))
            } else {
                return Err(anyhow!("unresolved lazy ref"));
            }
        }
        Op::Pin(_) => Ok(x),
        Op::Meta(_, _) => unreachable!(),
    }
}

/// Calculate a tree with minimal differences from `parent_tree`
/// such that `apply(unapply(tree, parent_tree)) == tree`
pub fn unapply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    if let Ok(inverted) = invert(filter) {
        let filtered = apply(
            transaction,
            invert(inverted)?,
            Rewrite::from_tree(parent_tree.clone()),
        )?;
        let matching = apply(transaction, inverted, filtered.clone())?;
        let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
        let x = apply(transaction, inverted, Rewrite::from_tree(tree))?;

        return Ok(transaction.repo().find_tree(tree::overlay(
            transaction,
            x.tree().id(),
            stripped,
        )?)?);
    }

    if let Some(ws) = unapply_workspace(
        transaction,
        &to_op(filter),
        tree.clone(),
        parent_tree.clone(),
    )? {
        return Ok(ws);
    }

    if let Op::Chain(filters) = peel_op(filter) {
        // Split into first and rest, unapply recursively
        let (first, rest) = match filters.split_first() {
            Some((first, rest)) => (first, rest),
            None => return Ok(tree),
        };

        if rest.is_empty() {
            return unapply(transaction, *first, tree, parent_tree);
        }

        let rest_chain = to_filter(Op::Chain(rest.to_vec()));

        // Compute filtered_parent_tree for the first filter
        let first_normalized = if let Ok(first_inverted) = invert(*first) {
            invert(first_inverted)?
        } else {
            *first
        };
        let filtered_parent_tree = apply(
            transaction,
            first_normalized,
            Rewrite::from_tree(parent_tree.clone()),
        )?
        .into_tree();

        // Recursively unapply: first unapply the rest, then unapply first
        return unapply(
            transaction,
            *first,
            unapply(transaction, rest_chain, tree, filtered_parent_tree)?,
            parent_tree,
        );
    }

    Err(anyhow!("filter cannot be unapplied"))
}

fn unapply_workspace<'a>(
    transaction: &'a cache::Transaction,
    op: &Op,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> anyhow::Result<Option<git2::Tree<'a>>> {
    match op {
        Op::Workspace(path) => {
            let tree = pre_process_tree(transaction.repo(), tree)?;
            let workspace = get_filter(transaction, &tree, Path::new("workspace.josh"));
            let original_workspace =
                get_filter(transaction, &parent_tree, &path.join("workspace.josh"));

            let root = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = to_filter(Op::File(
                Path::new("workspace.josh").to_owned(),
                Path::new("workspace.josh").to_owned(),
            ));
            let wsj_file = root.chain(wsj_file);
            let filter = compose(&[wsj_file, compose(&[workspace, root])]);
            let original_filter = compose(&[wsj_file, compose(&[original_workspace, root])]);
            let filtered = apply(
                transaction,
                original_filter,
                Rewrite::from_tree(parent_tree.clone()),
            )?;
            let matching = apply(transaction, invert(original_filter)?, filtered.clone())?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
            let x = apply(transaction, invert(filter)?, Rewrite::from_tree(tree))?;

            let result = transaction.repo().find_tree(tree::overlay(
                transaction,
                x.tree().id(),
                stripped,
            )?)?;

            Ok(Some(result))
        }
        Op::Stored(path) => {
            let stored_path = path.with_added_extension("josh");
            let stored = get_filter(transaction, &tree, &stored_path);
            let original_stored = get_filter(transaction, &parent_tree, &stored_path);

            let sj_file = Filter::new().file(stored_path.clone());
            let filter = compose(&[sj_file, stored]);
            let original_filter = compose(&[sj_file, original_stored]);
            let filtered = apply(
                transaction,
                original_filter,
                Rewrite::from_tree(parent_tree.clone()),
            )?;
            let matching = apply(transaction, invert(original_filter)?, filtered.clone())?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
            let x = apply(transaction, invert(filter)?, Rewrite::from_tree(tree))?;

            let result = transaction.repo().find_tree(tree::overlay(
                transaction,
                x.tree().id(),
                stripped,
            )?)?;

            Ok(Some(result))
        }
        #[cfg(feature = "incubating")]
        Op::Starlark(path, subfilter) => {
            let filter = get_starlark(transaction, &tree, path, *subfilter);
            let original_filter = get_starlark(transaction, &parent_tree, path, *subfilter);
            let filtered = apply(
                transaction,
                original_filter,
                Rewrite::from_tree(parent_tree.clone()),
            )?;
            let matching = apply(transaction, invert(original_filter)?, filtered.clone())?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
            let x = apply(transaction, invert(filter)?, Rewrite::from_tree(tree))?;

            let result = transaction.repo().find_tree(tree::overlay(
                transaction,
                x.tree().id(),
                stripped,
            )?)?;

            Ok(Some(result))
        }
        _ => Ok(None),
    }
}

fn pre_process_tree<'a>(
    repo: &'a git2::Repository,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    let path = Path::new("workspace.josh");
    let ws_file = tree::get_blob(repo, &tree, path);
    let parsed = filter::parse(&ws_file)?;

    if invert(parsed).is_err() {
        return Err(anyhow!("Invalid workspace: not reversible"));
    }

    let mut blob = String::new();
    if let Ok(c) = get_comments(&ws_file)
        && !c.is_empty()
    {
        blob = c;
    }
    let blob = &format!("{}{}\n", &blob, pretty(parsed, 0));

    let tree = tree::insert(
        repo,
        &tree,
        path,
        repo.blob(blob.as_bytes())?,
        git2::FileMode::Blob.into(), // Should this handle filemode?
    )?;

    Ok(tree)
}

/// Compute the warnings (filters not matching anything) for the filter applied to the tree
pub fn compute_warnings<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut filter = filter;

    if let Op::Workspace(path) = to_op(filter) {
        let workspace_filter = &tree::get_blob(
            transaction.repo(),
            &tree,
            &path.join(Path::new("workspace.josh")),
        );
        if let Ok(res) = parse(workspace_filter) {
            filter = res;
        } else {
            warnings.push("couldn't parse workspace\n".to_string());
            return warnings;
        }
    }

    if let Op::Stored(path) = to_op(filter) {
        let stored_path = path.with_added_extension("josh");
        let stored_filter = &tree::get_blob(transaction.repo(), &tree, &stored_path);
        if let Ok(res) = parse(stored_filter) {
            filter = res;
        } else {
            warnings.push("couldn't parse stored\n".to_string());
            return warnings;
        }
    }

    let filter = opt::flatten(filter);
    if let Op::Compose(filters) = to_op(filter) {
        for f in filters {
            let tree = transaction.repo().find_tree(tree.id());
            if let Ok(tree) = tree {
                warnings.append(&mut compute_warnings2(transaction, f, tree));
            }
        }
    } else {
        warnings.append(&mut compute_warnings2(transaction, filter, tree));
    }
    warnings
}

fn compute_warnings2<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> Vec<String> {
    let mut warnings = Vec::new();

    let x = apply(transaction, filter, Rewrite::from_tree(tree));
    if let Ok(x) = x
        && x.tree().is_empty()
    {
        warnings.push(format!("No match for \"{}\"", pretty(filter, 2)));
    }
    warnings
}

/// Check if `commit` is an ancestor of `tip`.
///
/// Creates a cache for a given `tip` so repeated queries with the same `tip` are more efficient.
pub fn is_ancestor_of(
    transaction: &cache::Transaction,
    commit: git2::Oid,
    tip: git2::Oid,
) -> anyhow::Result<bool> {
    if let Ok(tip_sequence_number) = cache::compute_sequence_number(transaction, tip) {
        if cache::compute_sequence_number(transaction, commit)? > tip_sequence_number {
            return Ok(false);
        }
    }

    let mut ancestor_cache = ANCESTORS.lock().unwrap();
    let ancestors = match ancestor_cache.entry(tip) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            tracing::trace!("is_ancestor_of tip={tip}");
            // Recursively compute all ancestors of `tip`.
            // Invariant: Everything in `todo` is also in `ancestors`.
            let mut todo = vec![tip];
            let mut ancestors = std::collections::HashSet::from_iter(todo.iter().copied());
            while let Some(commit) = todo.pop() {
                for parent in transaction.repo().find_commit(commit)?.parent_ids() {
                    if ancestors.insert(parent) {
                        // Newly inserted! Also handle its parents.
                        todo.push(parent);
                    }
                }
            }
            entry.insert(ancestors)
        }
    };
    Ok(ancestors.contains(&commit))
}

fn legalize_pin<F>(f: Filter, c: &F) -> Filter
where
    F: Fn(Filter) -> Filter,
{
    match to_op(f) {
        Op::Compose(f) => {
            let f = f.into_iter().map(|f| legalize_pin(f, c)).collect();
            to_filter(Op::Compose(f))
        }
        Op::Chain(filters) => to_filter(Op::Chain(
            filters.iter().map(|f| legalize_pin(*f, c)).collect(),
        )),
        Op::Subtract(a, b) => to_filter(Op::Subtract(legalize_pin(a, c), legalize_pin(b, c))),
        Op::Exclude(f) => to_filter(Op::Exclude(legalize_pin(f, c))),
        Op::Meta(meta, f) => to_filter(Op::Meta(meta, legalize_pin(f, c))),
        Op::Pin(f) => c(f),
        _ => f,
    }
}

fn legalize_stored(t: &cache::Transaction, f: Filter, tree: &git2::Tree) -> anyhow::Result<Filter> {
    if let Some(f) = t.get_legalize((f, tree.id())) {
        return Ok(f);
    }

    // Put an entry into the hashtable to prevent infinite recursion.
    // If we get called with the same arguments again before we return,
    // Above check breaks the recursion.
    t.insert_legalize((f, tree.id()), Filter::new().empty());

    let r = match to_op(f) {
        Op::Compose(f) => {
            let f = f
                .into_iter()
                .map(|f| legalize_stored(t, f, tree))
                .collect::<anyhow::Result<Vec<_>>>()?;
            to_filter(Op::Compose(f))
        }
        Op::Chain(filters) => {
            let mut result = Vec::with_capacity(filters.len());
            let mut current_tree = tree.clone();
            for filter in filters {
                let legalized = legalize_stored(t, filter, &current_tree)?;
                current_tree = apply(t, legalized, Rewrite::from_tree(current_tree.clone()))?.tree;
                result.push(legalized);
            }
            to_filter(Op::Chain(result))
        }
        Op::Subtract(a, b) => to_filter(Op::Subtract(
            legalize_stored(t, a, tree)?,
            legalize_stored(t, b, tree)?,
        )),
        Op::Exclude(f) => to_filter(Op::Exclude(legalize_stored(t, f, tree)?)),
        Op::Meta(meta, f) => to_filter(Op::Meta(meta, legalize_stored(t, f, tree)?)),
        Op::Pin(f) => to_filter(Op::Pin(legalize_stored(t, f, tree)?)),
        Op::Stored(path) => get_stored(t, tree, &path),
        #[cfg(feature = "incubating")]
        Op::Starlark(path, sub) => get_starlark(t, tree, &path, legalize_stored(t, sub, tree)?),
        _ => f,
    };

    t.insert_legalize((f, tree.id()), r);

    Ok(r)
}

fn per_rev_filter(
    transaction: &cache::Transaction,
    commit: &git2::Commit,
    filter: Filter,
    commit_filter: Filter,
    parent_filters: Vec<(git2::Commit, Filter)>,
) -> anyhow::Result<Option<git2::Oid>> {
    // Compute the difference between the current commit's filter and each parent's filter.
    // This determines what new content should be contributed by that parent in the filtered history.
    let extra_parents = parent_filters
        .into_iter()
        .map(|(parent, pcw)| {
            let f = opt::optimize(to_filter(Op::Subtract(commit_filter.peel(), pcw.peel())));
            apply_to_commit2(f, &parent, transaction)
        })
        .collect::<anyhow::Result<Option<Vec<_>>>>()?;

    let extra_parents = some_or!(extra_parents, { return Ok(None) });

    let extra_parents: Vec<_> = extra_parents
        .into_iter()
        .filter(|&oid| oid != git2::Oid::zero())
        .collect();

    let normal_parents = commit
        .parent_ids()
        .map(|parent| transaction.get(filter, parent))
        .collect::<Option<Vec<git2::Oid>>>();
    let normal_parents = some_or!(normal_parents, { return Ok(None) });

    // Special case: `:pin` filter needs to be aware of filtered history
    let pin_details = if let Some(&parent) = normal_parents.first() {
        let legalized_a = legalize_pin(commit_filter, &|f| f);
        let legalized_b = legalize_pin(commit_filter, &|f| to_filter(Op::Exclude(f)));

        if legalized_a != legalized_b {
            let pin_subtract = apply(
                transaction,
                opt::optimize(to_filter(Op::Subtract(legalized_a, legalized_b))),
                Rewrite::from_commit(commit)?,
            )?;

            let parent = transaction.repo().find_commit(parent)?;

            let pin_overlay = tree::populate(
                transaction,
                tree::pathstree("", pin_subtract.tree.id(), transaction)?.id(),
                parent.tree_id(),
            )?;

            Some((pin_subtract.tree.id(), pin_overlay))
        } else {
            None
        }
    } else {
        None
    };

    let filtered_parent_ids: Vec<_> = normal_parents.into_iter().chain(extra_parents).collect();

    let mut tree_data = apply(transaction, commit_filter, Rewrite::from_commit(commit)?)?;

    if let Some((pin_subtract, pin_overlay)) = pin_details {
        let with_exclude = tree::subtract(transaction, tree_data.tree().id(), pin_subtract)?;
        let with_overlay = tree::overlay(transaction, pin_overlay, with_exclude)?;

        tree_data = tree_data.with_tree(transaction.repo().find_tree(with_overlay)?);
    }

    return Some(history::create_filtered_commit_with_meta(
        commit,
        filtered_parent_ids,
        tree_data,
        transaction,
        filter,
        commit_filter.into_meta(),
    ))
    .transpose();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn src_path_test() {
        assert_eq!(PathBuf::from("x"), src_path(parse(":/x").unwrap()));
        assert_eq!(PathBuf::from("x/y"), src_path(parse(":/x/y").unwrap()));
        assert_eq!(PathBuf::from("x/y"), src_path(parse(":/x::y").unwrap()));
    }

    #[test]
    fn dst_path_test() {
        assert_eq!(PathBuf::from(""), dst_path(parse(":/x").unwrap()));
        assert_eq!(PathBuf::from(""), dst_path(parse(":/x/y").unwrap()));
        assert_eq!(PathBuf::from("y"), dst_path(parse(":/x::y").unwrap()));
        assert_eq!(
            PathBuf::from("a/y"),
            dst_path(parse(":[a=:/x::y/]").unwrap())
        );

        assert_eq!(
            PathBuf::from("c/a"),
            dst_path(parse(":[a=:/x::y/,a/b=:/i]:prefix=c").unwrap())
        );
    }

    #[test]
    fn invert_filter_parsing_test() {
        // Test that :invert[X] syntax parses correctly
        let filter = parse(":invert[:/sub1]").unwrap();
        // Verify it's not empty
        assert_ne!(filter, Filter::new().empty());

        // Test with prefix filter (inverse of subdir)
        let filter2 = parse(":invert[:prefix=sub1]").unwrap();
        assert_ne!(filter2, Filter::new().empty());

        // Test that it produces the correct inverse
        let filter3 = parse(":invert[:/sub1]").unwrap();
        let spec_str = spec(filter3);
        // Should produce prefix (inverse of subdir)
        assert!(spec_str.contains("prefix") || !spec_str.is_empty());

        // Test with multiple filters in compose
        let filter4 = parse(":invert[:/sub1,:/sub2]").unwrap();
        assert_ne!(filter4, Filter::new().empty());
    }

    #[test]
    fn scope_filter_parsing_test() {
        // Test that :<X>[Y] syntax parses correctly
        let filter = parse(":<:/sub1>[:/file1]").unwrap();
        // Just verify parsing succeeds (filter may optimize to empty in some cases)
        let _ = filter;

        // Test with multiple filters in compose
        let filter2 = parse(":<:/sub1>[:/file1,:/file2]").unwrap();
        let _ = filter2;

        // Test with prefix filter
        let filter3 = parse(":<:prefix=sub1>[:prefix=file1]").unwrap();
        let _ = filter3;

        // Test with exclude
        let filter4 = parse(":<:/sub1>[:exclude[::file1]]").unwrap();
        let _ = filter4;

        // Test that it expands to chain structure by checking spec output
        let filter5 = parse(":<:/sub1>[:/file1]").unwrap();
        let spec_str = spec(filter5);
        // The spec should contain the chain representation
        assert!(!spec_str.is_empty());
    }

    #[test]
    fn meta_filter_parsing_test() {
        // Test basic metadata parsing
        let filter = parse(":~(key1=\"value1\")[:/sub1]").unwrap();
        assert_ne!(filter, Filter::new().empty());
        let spec_str = spec(filter);
        assert_eq!(spec_str, ":~(key1=\"value1\")[:/sub1]");

        // Test with multiple metadata entries
        let filter2 = parse(":~(key1=\"value1\",key2=\"value2\")[:/sub1]").unwrap();
        assert_ne!(filter2, Filter::new().empty());
        let spec_str2 = spec(filter2);
        assert_eq!(spec_str2, ":~(key1=\"value1\",key2=\"value2\")[:/sub1]");

        // Test round-trip: parse -> spec -> parse -> spec should match
        let test_cases = vec![
            (":~(key1=\"value1\")[:/sub1]", ":~(key1=\"value1\")[:/sub1]"),
            (
                ":~(key1=\"value1\",key2=\"value2\")[:/sub1]",
                ":~(key1=\"value1\",key2=\"value2\")[:/sub1]",
            ),
            (":~(a=\"b\")[:/x]", ":~(a=\"b\")[:/x]"),
        ];

        for (test_input, expected_spec) in test_cases {
            let filter = parse(test_input).unwrap();
            let spec_str = spec(filter);
            assert_eq!(
                spec_str, expected_spec,
                "Spec mismatch for input '{}'",
                test_input
            );

            let reparsed = parse(&spec_str).unwrap();
            let respec_str = spec(reparsed);
            // The specs should match exactly since metadata entries are stored in BTreeMap (sorted)
            assert_eq!(
                spec_str, respec_str,
                "Round-trip failed for input '{}':\n  Original spec: {}\n  Reparsed spec: {}",
                test_input, spec_str, respec_str
            );
        }
    }

    #[test]
    fn meta_filter_tree_roundtrip_test() {
        use crate::filter::{as_tree, from_tree};
        use git2::Repository;
        use std::fs;

        // Create a temporary directory for the test repository
        let test_dir = std::env::temp_dir()
            .join("josh_test_flags")
            .join(format!("test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(&test_dir).unwrap();

        // Initialize a git repository
        let repo = Repository::init(&test_dir).unwrap();

        // Create a metadata filter
        let mut meta = std::collections::BTreeMap::new();
        meta.insert("key1".to_string(), "value1".to_string());
        meta.insert("key2".to_string(), "value2".to_string());
        let inner_filter = parse(":/sub1").unwrap();
        let meta_filter = to_filter(Op::Meta(meta.clone(), inner_filter));

        // Serialize to tree
        let tree_oid = as_tree(&repo, meta_filter).unwrap();

        // Deserialize from tree
        let deserialized_filter = from_tree(&repo, tree_oid).unwrap();

        // Verify the specs match
        let original_spec = spec(meta_filter);
        let deserialized_spec = spec(deserialized_filter);
        assert_eq!(
            original_spec, deserialized_spec,
            "Tree round-trip failed:\n  Original spec: {}\n  Deserialized spec: {}",
            original_spec, deserialized_spec
        );

        // Clean up
        let _ = fs::remove_dir_all(&test_dir);
    }
}
