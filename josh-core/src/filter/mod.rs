use super::*;
use anyhow::anyhow;
use gix_object::bstr::BString;
pub use josh_filter::check_experimental_features_enabled;
pub use josh_filter::experimental_features_enabled;

use std::path::Path;
use std::str::FromStr;
use std::sync::LazyLock;

// Re-export from josh-filter
pub use josh_filter::LinkMode;
pub use josh_filter::filter::MESSAGE_MATCH_ALL_REGEX;
pub use josh_filter::filter::index;
pub use josh_filter::filter::reachable_roots;
pub use josh_filter::filter::sequence_number;
pub use josh_filter::flang::parse::{get_comments, parse};
pub use josh_filter::opt;
pub use josh_filter::opt::invert;
pub use josh_filter::persist::{peel_filter, peel_op, to_filter, to_op, to_ops};
pub use josh_filter::{Filter, InsertContent, LazyRef, Op, RevMatch};
pub use josh_filter::{as_file, pretty, spec};

pub mod text;
pub mod tree;

pub fn as_tree(
    transaction: &cache::Transaction,
    filter: Filter,
) -> anyhow::Result<gix_hash::ObjectId> {
    josh_filter::persist::as_tree(transaction.odb(), filter)
}

pub fn from_tree(
    transaction: &cache::Transaction,
    tree_oid: gix_hash::ObjectId,
) -> anyhow::Result<Filter> {
    josh_filter::persist::from_tree(transaction.odb(), tree_oid)
}

static WORKSPACES: LazyLock<
    std::sync::Mutex<std::collections::HashMap<gix_hash::ObjectId, Filter>>,
> = LazyLock::new(Default::default);
static ANCESTORS: LazyLock<
    std::sync::Mutex<
        std::collections::HashMap<
            gix_hash::ObjectId,
            std::collections::HashSet<gix_hash::ObjectId>,
        >,
    >,
> = LazyLock::new(Default::default);

/// Clear the process-global workspace and ancestor caches.
pub fn clear_caches() {
    WORKSPACES.lock().unwrap().clear();
    ANCESTORS.lock().unwrap().clear();
}

// MESSAGE_MATCH_ALL_REGEX is now in josh-filter

// Filter type and builder methods are now in josh-filter
// Note: TryFrom<String> and From<Filter> for String implementations
// are not included here because they require parse/spec from josh-core
// and we cannot implement external traits for external types.
// These conversions should be done via parse() and spec() functions directly.

/// A signature override for `rewrite_commit`.
#[derive(Debug, Clone)]
pub enum SigRewrite {
    /// Re-serialize the signature from this name/email pair, keeping the base
    /// commit's timestamp (`:author`/`:committer`, whose specs carry no time).
    NameEmail(BString, BString),
    /// Raw signature field bytes, transplanted verbatim -- timestamp included,
    /// nothing parsed (the squash path copying the squash-result's signature).
    Raw(BString),
}

/// A pending commit rewrite in plain oid currency: the tree the rewritten commit will
/// point at, the base commit it derives from, and the metadata overrides. Tree objects are
/// never loaded on its behalf -- consumers read the tree through the transaction facade
/// when (and only when) they need its contents.
#[derive(Debug, Clone)]
pub struct Rewrite {
    tree: gix_hash::ObjectId,
    commit: gix_hash::ObjectId,
    pub author: Option<SigRewrite>,
    pub committer: Option<SigRewrite>,
    pub message: Option<BString>,
}

impl Rewrite {
    pub fn from_tree(tree: gix_hash::ObjectId) -> Self {
        Rewrite {
            tree,
            author: None,
            commit: gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            committer: None,
            message: None,
        }
    }

    pub fn from_tree_with_metadata(
        tree: gix_hash::ObjectId,
        author: Option<SigRewrite>,
        committer: Option<SigRewrite>,
        message: Option<BString>,
    ) -> Self {
        Rewrite {
            tree,
            author,
            commit: gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            committer,
            message,
        }
    }

    /// The metadata slots stay `None`: rewrite_commit keeps the base commit's raw
    /// signature and message bytes verbatim unless a filter overrides them, so the
    /// commit's author/committer/message are never parsed on the passthrough path.
    /// Only the tree id is taken from the header -- nothing validates the tree object
    /// here; a missing tree surfaces at the first actual tree read.
    pub fn from_commit_data(commit: &objects::CommitData) -> anyhow::Result<Self> {
        Ok(Rewrite {
            tree: commit.tree_id()?,
            commit: commit.id(),
            author: None,
            committer: None,
            message: None,
        })
    }

    pub fn with_author(self, author: (BString, BString)) -> Self {
        Rewrite {
            author: Some(SigRewrite::NameEmail(author.0, author.1)),
            ..self
        }
    }

    pub fn with_committer(self, committer: (BString, BString)) -> Self {
        Rewrite {
            committer: Some(SigRewrite::NameEmail(committer.0, committer.1)),
            ..self
        }
    }

    pub fn with_message(self, message: BString) -> Self {
        Rewrite {
            message: Some(message),
            ..self
        }
    }

    pub fn with_tree(self, tree: gix_hash::ObjectId) -> Self {
        Rewrite { tree, ..self }
    }

    pub fn tree_id(&self) -> gix_hash::ObjectId {
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
        Op::Exclude(filter) | Op::Select(filter) | Op::Pin(filter) => lazy_refs(*filter),
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
        Op::Downstack(LazyRef::Lazy(s)) => vec![s.to_owned()],
        Op::Downstack(_) => vec![],
        _ => vec![],
    };
    lr.sort();
    lr.dedup();
    lr
}

pub fn resolve_refs(
    refs: &std::collections::HashMap<String, gix_hash::ObjectId>,
    filter: Filter,
) -> Filter {
    to_filter(resolve_refs2(refs, &to_op(filter)))
}

fn resolve_refs2(refs: &std::collections::HashMap<String, gix_hash::ObjectId>, op: &Op) -> Op {
    match op {
        Op::Compose(filters) => {
            Op::Compose(filters.iter().map(|f| resolve_refs(refs, *f)).collect())
        }
        Op::Exclude(filter) => Op::Exclude(resolve_refs(refs, *filter)),
        Op::Select(filter) => Op::Select(resolve_refs(refs, *filter)),
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
        Op::Downstack(LazyRef::Lazy(s)) => {
            if let Some(res) = refs.get(s) {
                Op::Downstack(LazyRef::Resolved(*res))
            } else {
                op.clone()
            }
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

/// Fully flatten a (possibly nested) chain filter into an ordered list of
/// primitive (non-chain) steps with their accumulated metadata.
///
/// The outer `Meta` of a chain is merged into each step before recursing.
/// When a step is itself a meta-wrapped chain the process repeats,
/// accumulating metadata all the way down to the leaf filters.
///
/// This makes all intermediate cache-key filters visible so they can be stored
/// as git refs for the distributed cache.
///
/// Examples:
/// ```text
/// Chain([A, B, C])
///   → [A, B, C]
///
/// Meta(m, Chain([A, B, C]))
///   → [Meta(m,A), Meta(m,B), Meta(m,C)]
///
/// Meta(m1, Chain([Meta(m2, Chain([A, B])), C]))
///   → [Meta(m1∪m2,A), Meta(m1∪m2,B), Meta(m1,C)]
/// ```
pub fn flatten_chain(filter: Filter) -> Vec<Filter> {
    fn expand(
        filter: Filter,
        outer_meta: std::collections::BTreeMap<String, String>,
    ) -> Vec<Filter> {
        // Merge this filter's own meta with the outer meta.
        // The filter's own meta takes precedence (inner wins on key collision).
        let mut meta = filter.into_meta();
        for (k, v) in outer_meta {
            meta.entry(k).or_insert(v);
        }
        match peel_op(filter) {
            Op::Chain(steps) => steps
                .into_iter()
                .flat_map(|f| expand(f, meta.clone()))
                .collect(),
            _ => vec![propagate_meta(filter.peel(), &meta)],
        }
    }
    expand(filter, std::collections::BTreeMap::new())
}

fn propagate_meta(filter: Filter, meta: &std::collections::BTreeMap<String, String>) -> Filter {
    let mut f = filter;
    for (k, v) in meta {
        f = f.with_meta(k, v);
    }
    f
}

/// Calculate the filtered commit for `commit`. This can take some time if done
/// for the first time and thus should generally be done asynchronously.
pub fn apply_to_commit(
    filter: Filter,
    commit: gix_hash::ObjectId,
    transaction: &cache::Transaction,
) -> anyhow::Result<gix_hash::ObjectId> {
    let filter = opt::optimize(filter);
    loop {
        let filtered = apply_to_commit2(filter, commit, transaction)?;

        if let Some(id) = filtered {
            return Ok(id);
        }

        let missing = transaction.get_missing()?;

        let max_level = missing.last().map(|(w, _, _)| *w).unwrap_or(0);

        for (level, filter, input) in missing.iter().rev() {
            log::info!("MISSING {} {} {}", level, input, filter::spec(*filter));
        }

        for (level, filter, input) in missing.into_iter().rev() {
            if level != max_level {
                break;
            }
            transaction.set_nesting(level + 1);
            history::walk2(filter, input, transaction)?;
        }
    }
}

// Handle workspace.josh files that contain ":workspace=..." as their only filter as
// a "redirect" to that other workspace. We chain an exclude of the redirecting workspace
// in front to prevent infinite recursion.
fn resolve_workspace_redirect(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    reader: &tree::TreeReader,
    path: &Path,
) -> Option<(Filter, std::path::PathBuf)> {
    let f = parse(&tree::get_blob_at(
        transaction,
        odb,
        reader,
        &path.join("workspace.josh"),
    ))
    .unwrap_or_else(|_| to_filter(Op::Empty));

    if let Op::Workspace(p) = peel_op(f) {
        let inner = to_filter(Op::Exclude(Filter::new().file(path))).chain(f.peel());
        Some((propagate_meta(inner, &f.into_meta()), p))
    } else {
        None
    }
}

fn get_workspace(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
    reader: &tree::TreeReader,
    path: &Path,
) -> Filter {
    let wsj_file = Filter::new().file("workspace.josh");
    let base = to_filter(Op::Subdir(path.to_owned()));
    let wsj_file = base.chain(wsj_file);
    compose(&[
        wsj_file,
        compose(&[
            get_filter(transaction, odb, tree, reader, &path.join("workspace.josh")),
            base,
        ]),
    ])
}

fn get_stored(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
    reader: &tree::TreeReader,
    path: &Path,
) -> Filter {
    let stored_path = path.with_added_extension("josh");
    let sj_file = Filter::new().file(stored_path.clone());
    compose(&[
        sj_file,
        get_filter(transaction, odb, tree, reader, &stored_path),
    ])
}

fn get_starlark(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
    reader: &tree::TreeReader,
    path: &Path,
    subfilter: Filter,
) -> Filter {
    let star_path = path.with_added_extension("star");
    let script = tree::get_blob_at(transaction, odb, reader, &star_path);
    let filtered_tree = match apply(transaction, subfilter, Rewrite::from_tree(tree)) {
        Ok(rw) => rw.tree_id(),
        Err(_) => return to_filter(Op::Empty),
    };
    match josh_starlark::evaluate(&script, filtered_tree, odb) {
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

fn get_filter(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
    reader: &tree::TreeReader,
    path: &Path,
) -> Filter {
    let ws_path = normalize_path(path);
    // One descent resolves both the WORKSPACES cache key (the workspace.josh entry oid)
    // and the blob to parse.
    let ws_id = match tree::get_path_entry_at(transaction, odb, reader, &ws_path) {
        Ok(Some(entry)) => entry.oid,
        _ => {
            return to_filter(Op::Empty);
        }
    };
    let ws_blob = tree::blob_text(odb, ws_id);

    if let Some(f) = WORKSPACES.lock().unwrap().get(&ws_id) {
        *f
    } else {
        let f = parse(&ws_blob).unwrap_or_else(|_| to_filter(Op::Empty));
        let f = legalize_stored(transaction, odb, f, tree, reader)
            .unwrap_or_else(|_| to_filter(Op::Empty));

        let f = if invert(f).is_ok() {
            f
        } else {
            to_filter(Op::Empty)
        };
        WORKSPACES.lock().unwrap().insert(ws_id, f);
        f
    }
}

fn read_josh_link(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    reader: &tree::TreeReader,
    root: &std::path::Path,
    filename: &str,
) -> Option<Filter> {
    use anyhow::Context;

    let link_path = root.join(filename);
    let link_entry = tree::get_path_entry_at(transaction, odb, reader, &link_path)
        .ok()
        .flatten()?;
    let link_blob = tree::blob_bytes(odb, link_entry.oid)?;
    let b = std::str::from_utf8(&link_blob)
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
    commit_id: gix_hash::ObjectId,
    filters: &[(RevMatch, LazyRef, Filter)],
) -> anyhow::Result<Filter> {
    // First match wins - iterate in order
    for (match_op, filter_tip_ref, startfilter) in filters.iter() {
        let filter_tip = if let LazyRef::Resolved(filter_tip) = filter_tip_ref {
            filter_tip.to_owned()
        } else {
            return Err(anyhow!("unresolved lazy ref"));
        };
        if match_op != &RevMatch::Default && !transaction.odb().contains(filter_tip) {
            return Err(anyhow!("`:rev(...)` with nonexistent OID: {}", filter_tip));
        }
        let matches = match match_op {
            RevMatch::AncestorStrict => {
                // `<` - matches if commit is ancestor of tip AND commit != tip (strict)

                is_ancestor_of(transaction, commit_id, filter_tip)? && commit_id != filter_tip
            }
            RevMatch::AncestorInclusive => {
                // `<=` - matches if commit is ancestor of tip OR commit == tip (inclusive)

                is_ancestor_of(transaction, commit_id, filter_tip)?
            }
            RevMatch::Equal => {
                // `==` - matches if commit == tip

                commit_id == filter_tip
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
    commit_id: gix_hash::ObjectId,
    transaction: &cache::Transaction,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let op = peel_op(filter);

    if filter == Filter::new() {
        return Ok(Some(commit_id));
    }

    // First match: oid-only fast paths, no object loads. The commit bytes are only read
    // after the memo gate below, so memo hits stay zero-I/O.
    match &op {
        Op::Empty => return Ok(Some(gix_hash::ObjectId::null(gix_hash::Kind::Sha1))),

        Op::Chain(_) => {
            let mut current_oid = commit_id;
            for f in flatten_chain(filter) {
                if current_oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    break;
                }
                let r = some_or!(apply_to_commit2(f, current_oid, transaction)?, {
                    return Ok(None);
                });
                current_oid = r;
            }
            return Ok(Some(current_oid));
        }
        Op::Squash(None) => {
            let odb = transaction.odb();
            let commit = objects::CommitData::read(odb, commit_id)?;
            odb.read_header(commit.tree_id()?)?;
            return Some(history::rewrite_commit(
                odb,
                &commit,
                &[],
                Rewrite::from_commit_data(&commit)?,
                history::GpgsigMode::Remove,
            ))
            .transpose();
        }
        Op::Downstack(LazyRef::Resolved(base)) => {
            if let Some(oid) = transaction.get(filter, commit_id)? {
                return Ok(Some(oid));
            }
            let new_oid = downstack(transaction, commit_id, base.to_owned())?;
            transaction.insert(filter, commit_id, new_oid, false)?;
            return Ok(Some(new_oid));
        }
        Op::Downstack(LazyRef::Lazy(_)) => {
            return Err(anyhow!("`:_=...` with unresolved base ref"));
        }
        _ => {
            if let Some(oid) = transaction.get(filter, commit_id)? {
                return Ok(Some(oid));
            }
            // Continue to process the filter if not cached
        }
    };

    let odb = transaction.odb();
    let commit = objects::CommitData::read(odb, commit_id)?;
    // Validate the commit's tree before any filter work (a header probe: no decompression,
    // no parse). Without this gate, an apply over a partially unreadable input can buffer
    // fresh objects into the store before a later read aborts the walk, and the partial
    // write would change the next flush's pack.
    odb.read_header(commit.tree_id()?)?;

    let rewrite_data = match &op {
        Op::Squash(Some(ids)) => {
            if let Some(sq) = ids.get(&LazyRef::Resolved(commit.id())) {
                let oid = if let Some(oid) = apply_to_commit2(
                    filter::Filter::new().squash(None).chain(*sq),
                    commit_id,
                    transaction,
                )? {
                    oid
                } else {
                    return Ok(None);
                };

                // Transplant the squash result's metadata verbatim onto the rewrite of the
                // original commit (which must stay the base: memoization keys on its id).
                // Timestamps are the base's own anyway -- no filter alters them.
                let rc = objects::CommitData::read(odb, oid)?;
                let rcr = rc.parsed()?;
                Rewrite::from_tree_with_metadata(
                    rc.tree_id()?,
                    Some(SigRewrite::Raw(rcr.author.to_owned())),
                    Some(SigRewrite::Raw(rcr.committer.to_owned())),
                    Some(rcr.message.to_owned()),
                )
            } else {
                if let Some(parent) = commit.first_parent_id() {
                    return Ok(if let Some(fparent) = transaction.get(filter, parent)? {
                        Some(history::drop_commit(
                            commit.id(),
                            vec![fparent],
                            transaction,
                            filter,
                        )?)
                    } else {
                        None
                    });
                }
                return Ok(Some(history::drop_commit(
                    commit.id(),
                    vec![],
                    transaction,
                    filter,
                )?));
            }
        }
        Op::Prune => {
            let p: Vec<_> = commit.parent_ids().collect();

            if p.len() > 1 {
                let parent = some_or!(transaction.get(filter, p[0])?, {
                    return Ok(None);
                });

                let parent_tree = history::filtered_parent_tree_id(transaction, parent)?;

                if parent_tree == commit.tree_id()? {
                    return Ok(Some(history::drop_commit(
                        commit.id(),
                        vec![parent],
                        transaction,
                        filter,
                    )?));
                }
            }

            Rewrite::from_commit_data(&commit)?
        }
        Op::Export => {
            check_experimental_features_enabled("export filter")?;
            let filtered_parent_ids = {
                commit
                    .parent_ids()
                    .map(|x| transaction.get(filter, x))
                    .collect::<anyhow::Result<Option<_>>>()?
            };

            let mut filtered_parent_ids: Vec<gix_hash::ObjectId> =
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

            let tree = commit.tree_id()?;
            // The link probe below folds every failure into "no link", so an unreadable
            // or unparseable commit tree must error here.
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            if let Some(link_file) = read_josh_link(
                transaction,
                odb,
                &tree_reader,
                &std::path::PathBuf::new(),
                ".link.josh",
            ) {
                if let Some(commit_str) = link_file.get_meta("commit") {
                    if let Ok(commit_oid) = gix_hash::ObjectId::from_str(&commit_str) {
                        if filtered_parent_ids.contains(&commit_oid) {
                            while filtered_parent_ids[0] != commit_oid {
                                filtered_parent_ids.rotate_right(1);
                            }
                        }
                    }
                }
            }

            return Some(history::create_filtered_commit(
                &commit,
                filtered_parent_ids,
                apply(transaction, filter, Rewrite::from_commit_data(&commit)?)?,
                transaction,
                filter,
            ))
            .transpose();
        }
        Op::Unlink => {
            check_experimental_features_enabled("unlink filter")?;
            use crate::link::find_link_files;

            let filtered_parent_ids = {
                commit
                    .parent_ids()
                    .map(|x| transaction.get(filter, x))
                    .collect::<anyhow::Result<Option<_>>>()?
            };

            let mut filtered_parent_ids: Vec<gix_hash::ObjectId> =
                some_or!(filtered_parent_ids, { return Ok(None) });

            let mut link_parents = vec![];
            for (link_path, link_file) in find_link_files(odb, commit.tree_id()?)?.into_iter() {
                if let Some(commit_str) = link_file.get_meta("commit") {
                    if let Ok(commit_oid) = gix_hash::ObjectId::from_str(&commit_str) {
                        if let Some(cmt) =
                            transaction.get(to_filter(Op::Prefix(link_path)), commit_oid)?
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

            let new_tree = apply(transaction, filter, Rewrite::from_commit_data(&commit)?)?;

            filtered_parent_ids.retain(|c| !link_parents.contains(c));

            return Some(history::create_filtered_commit(
                &commit,
                filtered_parent_ids,
                new_tree,
                transaction,
                filter,
            ))
            .transpose();
        }
        Op::Link(mode) => {
            check_experimental_features_enabled("link filter")?;
            let tree = commit.tree_id()?;
            // An unreadable commit tree is a hard error -- the retain probes and link
            // reads below fold their failures -- and they all share this one parse.
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let mut roots = get_link_roots(transaction, odb, tree)?;

            // A root commit (no first parent) keeps every root; when a first
            // parent is present its tree must be readable (the retain closure below cannot
            // error).
            if let Some(parent) = commit.first_parent_id() {
                let parent_tree = git::read_tree_id(odb, parent)?;
                let parent_reader = tree::read_tree(transaction, odb, parent_tree)?;
                roots.retain(|root| {
                    match (
                        tree::get_path_entry_at(transaction, odb, &tree_reader, root),
                        tree::get_path_entry_at(transaction, odb, &parent_reader, root),
                    ) {
                        (Ok(Some(a)), Ok(Some(b))) if a.oid == b.oid => false,
                        _ => true,
                    }
                });
            }

            let all_links = links_from_roots(transaction, odb, &tree_reader, roots)?;

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
                apply(transaction, filter, Rewrite::from_commit_data(&commit)?)?
            } else {
                let normal_parents = commit
                    .parent_ids()
                    .map(|parent| transaction.get(filter, parent))
                    .collect::<anyhow::Result<Option<Vec<gix_hash::ObjectId>>>>()?;

                let normal_parents = some_or!(normal_parents, { return Ok(None) });

                let extra_parents = {
                    let mut extra_parents = vec![];
                    for (root, _link_file) in embedded_links {
                        let embeding = some_or!(
                            apply_to_commit2(
                                Filter::new().message("{@}").file(root.join(".link.josh")),
                                commit_id,
                                transaction
                            )?,
                            {
                                return Ok(None);
                            }
                        );

                        let f = to_filter(Op::Embed(root));

                        let r = some_or!(apply_to_commit2(f, embeding, transaction)?, {
                            return Ok(None);
                        });

                        extra_parents.push(r);
                    }

                    extra_parents
                };

                let filtered_tree =
                    apply(transaction, filter, Rewrite::from_commit_data(&commit)?)?;
                let filtered_parent_ids = normal_parents
                    .into_iter()
                    .chain(extra_parents)
                    .collect::<Vec<_>>();

                return Some(history::create_filtered_commit(
                    &commit,
                    filtered_parent_ids,
                    filtered_tree,
                    transaction,
                    filter,
                ))
                .transpose();
            }
        }
        Op::Workspace(ws_path) => {
            // The get_* helpers return a bare Filter and would fold an unreadable tree to
            // Op::Empty, so bad input must error here; every probe below shares the read.
            let tree = commit.tree_id()?;
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            if let Some((redirect, _)) =
                resolve_workspace_redirect(transaction, odb, &tree_reader, ws_path)
            {
                if let Some(r) = apply_to_commit2(redirect, commit_id, transaction)? {
                    transaction.insert(filter, commit.id(), r, true)?;
                    return Ok(Some(r));
                } else {
                    return Ok(None);
                }
            }

            let commit_filter = get_workspace(transaction, odb, tree, &tree_reader, ws_path);

            let parent_filters = commit
                .parent_ids()
                .map(|parent| {
                    let tree_id = git::read_tree_id(odb, parent)?;
                    let reader = tree::read_tree(transaction, odb, tree_id)?;
                    let pcw = get_workspace(transaction, odb, tree_id, &reader, ws_path);
                    Ok((parent, pcw))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, &commit, filter, commit_filter, parent_filters);
        }
        Op::Stored(s_path) => {
            let tree = commit.tree_id()?;
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let commit_filter = get_stored(transaction, odb, tree, &tree_reader, s_path);

            let parent_filters = commit
                .parent_ids()
                .map(|parent| {
                    let tree_id = git::read_tree_id(odb, parent)?;
                    let reader = tree::read_tree(transaction, odb, tree_id)?;
                    let pcs = get_stored(transaction, odb, tree_id, &reader, s_path);
                    Ok((parent, pcs))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, &commit, filter, commit_filter, parent_filters);
        }
        Op::Starlark(s_path, s_subfilter) => {
            check_experimental_features_enabled("Starlark filter")?;
            let tree = commit.tree_id()?;
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let commit_filter =
                get_starlark(transaction, odb, tree, &tree_reader, s_path, *s_subfilter);

            let parent_filters = commit
                .parent_ids()
                .map(|parent| {
                    let tree_id = git::read_tree_id(odb, parent)?;
                    let reader = tree::read_tree(transaction, odb, tree_id)?;
                    let pcs =
                        get_starlark(transaction, odb, tree_id, &reader, s_path, *s_subfilter);
                    Ok((parent, pcs))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, &commit, filter, commit_filter, parent_filters);
        }
        Op::Rev(filters) => {
            let commit_filter = get_rev_filter(transaction, commit.id(), filters)?;

            let parent_filters = commit
                .parent_ids()
                .map(|parent| {
                    let pcw = get_rev_filter(transaction, parent, filters)?;
                    Ok((parent, pcw))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, &commit, filter, commit_filter, parent_filters);
        }
        Op::Fold => {
            let filtered_parent_ids = commit
                .parent_ids()
                .map(|x| transaction.get(filter, x))
                .collect::<anyhow::Result<Option<Vec<_>>>>()?;

            let filtered_parent_ids = some_or!(filtered_parent_ids, { return Ok(None) });

            let trees: Vec<gix_hash::ObjectId> = filtered_parent_ids
                .iter()
                .map(|x| history::filtered_parent_tree_id(transaction, *x))
                .collect::<anyhow::Result<_>>()?;

            let mut filtered_tree = commit.tree_id()?;

            for t in trees {
                filtered_tree = tree::overlay(transaction, filtered_tree, t)?;
            }

            Rewrite::from_commit_data(&commit)?.with_tree(filtered_tree)
        }
        Op::Hook(hook) => {
            let commit_filter = transaction.lookup_filter_hook(hook, commit.id())?;

            let parent_filters = commit
                .parent_ids()
                .map(|parent| {
                    let pcw = transaction.lookup_filter_hook(hook, parent)?;
                    Ok((parent, pcw))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            return per_rev_filter(transaction, &commit, filter, commit_filter, parent_filters);
        }
        Op::Unapply(target, uf) => {
            check_experimental_features_enabled("unapply filter")?;
            if let LazyRef::Resolved(target) = target {
                /* dbg!(target); */
                let target = objects::CommitData::read(odb, target.to_owned())?;
                // Only a root commit (no first parent) skips link detection; a
                // first parent that is present must be readable.
                if let Some(parent_id) = target.first_parent_id() {
                    let parent = objects::CommitData::read(odb, parent_id)?;
                    let ptree = apply(transaction, *uf, Rewrite::from_commit_data(&parent)?)?;
                    // The link probe folds every failure into "no link", so an unreadable
                    // filtered tree must error here.
                    let ptree_id = ptree.tree_id();
                    let ptree_reader = tree::read_tree(transaction, odb, ptree_id)?;
                    if let Some(link) = read_josh_link(
                        transaction,
                        odb,
                        &ptree_reader,
                        &std::path::PathBuf::new(),
                        ".link.josh",
                    ) {
                        if let Some(commit_str) = link.get_meta("commit") {
                            if let Ok(link_commit) = gix_hash::ObjectId::from_str(&commit_str) {
                                if commit.id() == link_commit {
                                    let unapply =
                                        to_filter(Op::Unapply(LazyRef::Resolved(parent.id()), *uf));
                                    let r = some_or!(transaction.get(unapply, link_commit)?, {
                                        return Ok(None);
                                    });
                                    transaction.insert(filter, commit.id(), r, true)?;
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
                Rewrite::from_commit_data(&commit)?, /* Rewrite::from_commit_data(&commit)?.with_parents(filtered_parent_ids), */
            )?
        }
        Op::Embed(path) => {
            check_experimental_features_enabled("embed filter")?;

            let tree = commit.tree_id()?;
            // The link probe below folds every failure into "no link", so an unreadable
            // or unparseable commit tree must error here.
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            if let Some(link) = read_josh_link(transaction, odb, &tree_reader, path, ".link.josh") {
                let subdir = filter::invert(link.peel())?;
                let unapply = to_filter(Op::Unapply(LazyRef::Resolved(commit.id()), subdir));
                if let Some(commit_str) = link.get_meta("commit") {
                    if let Ok(commit_oid) = gix_hash::ObjectId::from_str(&commit_str) {
                        let r = some_or!(transaction.get(unapply, commit_oid)?, {
                            return Ok(None);
                        });
                        transaction.insert(filter, commit.id(), r, true)?;
                        return Ok(Some(r));
                    }
                }
            }
            return Ok(Some(gix_hash::ObjectId::null(gix_hash::Kind::Sha1)));
        }

        _ => apply(transaction, filter, Rewrite::from_commit_data(&commit)?)?,
    };

    let tree_data = rewrite_data;

    let filtered_parent_ids = {
        commit
            .parent_ids()
            .map(|x| transaction.get(filter, x))
            .collect::<anyhow::Result<Option<_>>>()?
    };

    let filtered_parent_ids = some_or!(filtered_parent_ids, { return Ok(None) });

    Some(history::create_filtered_commit(
        &commit,
        filtered_parent_ids,
        tree_data,
        transaction,
        filter,
    ))
    .transpose()
}

fn extract_submodule_commits(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<
    std::collections::BTreeMap<
        std::path::PathBuf,
        (gix_hash::ObjectId, crate::submodules::ParsedSubmoduleEntry),
    >,
> {
    use crate::submodules::{ParsedSubmoduleEntry, parse_gitmodules};
    // One hoisted parse for the .gitmodules probe and every per-submodule probe; an
    // unreadable tree is a hard error (the probes below fold their failures).
    let tree_reader = tree::read_tree(transaction, odb, tree)?;

    let gitmodules_content = tree::get_blob_at(
        transaction,
        odb,
        &tree_reader,
        std::path::Path::new(".gitmodules"),
    );

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
        (gix_hash::ObjectId, ParsedSubmoduleEntry),
    > = std::collections::BTreeMap::new();

    for parsed in submodule_entries {
        let submodule_path = parsed.path.clone();
        if let Ok(Some(entry)) =
            tree::get_path_entry_at(transaction, odb, &tree_reader, &submodule_path)
        {
            if entry.mode.is_commit() {
                let commit_oid = entry.oid;
                submodule_commits.insert(submodule_path, (commit_oid, parsed));
            }
        }
    }

    Ok(submodule_commits)
}

fn get_link_roots(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let link_filter = to_filter(Op::pattern("**/.link.josh")?);
    let link_tree = apply_impl(transaction, odb, link_filter, Rewrite::from_tree(tree))?;

    // Stored-order preorder is load-bearing: the roots order feeds the extra-parents
    // order of created merge commits.
    let mut roots = vec![];
    objects::walk_tree_preorder(odb, link_tree.tree_id(), &mut |root, entry| {
        let root = std::path::PathBuf::from(root);
        if &entry.filename[..] == b".link.josh" {
            roots.push(root);
        }
        Ok(())
    })?;

    Ok(roots)
}

fn links_from_roots(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    reader: &tree::TreeReader,
    roots: Vec<std::path::PathBuf>,
) -> anyhow::Result<Vec<(std::path::PathBuf, Filter)>> {
    let mut v = vec![];
    for root in roots {
        if let Some(link_filter) = read_josh_link(transaction, odb, reader, &root, ".link.josh") {
            v.push((root, link_filter));
        }
    }
    Ok(v)
}

/// Filter a single tree. This does not involve walking history and is thus fast in most cases.
pub fn apply(
    transaction: &cache::Transaction,
    filter: Filter,
    x: Rewrite,
) -> anyhow::Result<Rewrite> {
    // One facade handle for the whole (possibly deeply recursive) application -- building it
    // per recursion level would cost an FFI round-trip each.
    let odb = transaction.odb();
    apply_impl(transaction, odb, filter, x)
}

fn apply_impl(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    filter: Filter,
    x: Rewrite,
) -> anyhow::Result<Rewrite> {
    let op = peel_op(filter);
    match &op {
        Op::Nop => Ok(x),
        Op::Empty => Ok(x.with_tree(tree::empty_id())),
        Op::Fold => Ok(x),
        Op::Squash(None) => Ok(x),
        Op::Author(author, email) => {
            Ok(x.with_author((author.clone().into(), email.clone().into())))
        }
        Op::Committer(author, email) => {
            Ok(x.with_committer((author.clone().into(), email.clone().into())))
        }
        Op::Squash(Some(_)) => Err(anyhow!("not applicable to tree: squash")),
        Op::Message(m, r) => {
            // Rewriting a message leaves the tree alone, so with neither a commit nor a
            // message to transform this is identity, like the other history-only filters.
            if x.commit == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) && x.message.is_none() {
                return Ok(x);
            }

            let tree = x.tree_id();
            let tree_id = tree.to_string();
            let commit = x.commit;
            let commit_id = commit.to_string();

            // The template is the one consumer that needs the message as text, so the UTF-8
            // decode happens here, and only here. A commit that cannot be read or parsed is
            // an error rather than an empty message.
            let message = if let Some(ref m) = x.message {
                std::str::from_utf8(m.as_ref())?.to_string()
            } else {
                let base = objects::CommitData::read(odb, commit)?;
                std::str::from_utf8(base.message_raw()?)?.to_string()
            };

            // One lazily-read tree shared by all template keys: a key-less template reads
            // nothing, and an unreadable tree keeps the keys' tolerance ("" / zero oid).
            let tree_reader = std::cell::OnceCell::new();
            let reader = || {
                tree_reader
                    .get_or_init(|| tree::read_tree(transaction, odb, tree).ok())
                    .as_ref()
            };

            Ok(x.with_message(
                text::transform_with_template(r, m, &message, |key: &str| -> Option<String> {
                    match key {
                        "#" => Some(tree_id.clone()),
                        "@" => Some(commit_id.clone()),
                        key if key.starts_with("/") => Some(
                            reader()
                                .map(|p| {
                                    tree::get_blob_at(
                                        transaction,
                                        odb,
                                        p,
                                        std::path::Path::new(&key[1..]),
                                    )
                                })
                                .unwrap_or_default(),
                        ),

                        key if key.starts_with("#") => Some(
                            reader()
                                .and_then(|p| {
                                    tree::get_path_entry_at(
                                        transaction,
                                        odb,
                                        p,
                                        std::path::Path::new(&key[1..]),
                                    )
                                    .ok()
                                    .flatten()
                                })
                                .map(|e| e.oid)
                                .unwrap_or(gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
                                .to_string(),
                        ),
                        _ => None,
                    }
                })?
                .into(),
            ))
        }
        Op::Prune => Ok(x),
        Op::Adapt(adapter) => {
            let mut result_tree = x.tree_id();
            match adapter.as_ref() {
                "submodules" => {
                    // Extract submodule commits
                    let submodule_commits =
                        extract_submodule_commits(transaction, odb, result_tree)?;

                    // Process each submodule commit
                    for (submodule_path, (commit_oid, meta)) in submodule_commits {
                        let prefix_filter = Filter::new().prefix(&submodule_path);

                        // Create a filter with metadata
                        let link_filter = prefix_filter
                            .with_meta("remote", meta.url.clone())
                            .with_meta("target", "HEAD")
                            .with_meta("commit", commit_oid.to_string())
                            .with_meta("mode", josh_filter::LinkMode::Pointer.to_string());
                        let link_content = as_file(link_filter, 0);

                        result_tree = tree::insert_oid(
                            odb,
                            result_tree,
                            &submodule_path.join(".link.josh"),
                            odb.write(gix_object::Kind::Blob, link_content.as_bytes()),
                            0o0100644,
                        )?;
                    }

                    // Remove .gitmodules file by setting it to zero OID
                    result_tree = tree::insert_oid(
                        odb,
                        result_tree,
                        std::path::Path::new(".gitmodules"),
                        gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                        0o0100644,
                    )?;
                }
                _ => return Err(anyhow!("unknown adapter {:?}", adapter)),
            }

            Ok(x.with_tree(result_tree))
        }
        Op::Export => {
            let tree = x.tree_id();
            Ok(x.with_tree(tree::insert_oid(
                odb,
                tree,
                std::path::Path::new(".link.josh"),
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                0o0100644,
            )?))
        }
        Op::Unlink => {
            check_experimental_features_enabled("unlink filter")?;
            use crate::link::find_link_files;
            let mut result_tree = x.tree;
            for (link_path, link_file) in find_link_files(odb, result_tree)?.iter() {
                result_tree = tree::insert_oid(
                    odb,
                    result_tree,
                    link_path,
                    gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    0o0100644,
                )?;

                // The link_file is already a filter with metadata, just serialize it
                let link_content = as_file(*link_file, 0);

                result_tree = tree::insert_oid(
                    odb,
                    result_tree,
                    &link_path.join(".link.josh"),
                    odb.write(gix_object::Kind::Blob, link_content.as_bytes()),
                    0o0100644,
                )?;
            }
            Ok(x.with_tree(result_tree))
        }
        Op::Link(mode) => {
            let tree = x.tree_id();
            // An unreadable input tree is a hard error; the hoisted parse serves the
            // link reads below.
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let roots = get_link_roots(transaction, odb, tree)?;
            let v = links_from_roots(transaction, odb, &tree_reader, roots)?;
            let mut result_tree = tree;

            for (root, link_file) in v {
                // Get commit from metadata
                let commit_oid = link_file
                    .get_meta("commit")
                    .and_then(|s| gix_hash::ObjectId::from_str(&s).ok())
                    .ok_or_else(|| anyhow!("Link file missing commit metadata"))?;

                let submodule_tree = git::read_tree_id(odb, commit_oid)?;
                let inner_filter = link_file.peel();
                let submodule_tree = apply_impl(
                    transaction,
                    odb,
                    inner_filter,
                    Rewrite::from_tree(submodule_tree),
                )
                .unwrap();

                result_tree = tree::overlay(transaction, result_tree, submodule_tree.tree_id())?;
                let effective_mode = mode.clone().unwrap_or_else(|| {
                    link_file
                        .get_meta("mode")
                        .and_then(|s| josh_filter::LinkMode::parse(&s).ok())
                        .unwrap_or(josh_filter::LinkMode::Pointer)
                });
                let link_content =
                    as_file(link_file.with_meta("mode", effective_mode.to_string()), 0);

                result_tree = tree::insert_oid(
                    odb,
                    result_tree,
                    &root.join(".link.josh"),
                    odb.write(gix_object::Kind::Blob, link_content.as_bytes()),
                    0o0100644,
                )?;
            }

            Ok(x.with_tree(result_tree))
        }
        Op::Rev(_) => Err(anyhow!("not applicable to tree: rev")),
        Op::RegexReplace(replacements) => {
            let mut t = x.tree_id();
            for (regex, replacement) in replacements {
                t = tree::regex_replace(t, regex, replacement, transaction)?;
            }
            Ok(x.with_tree(t))
        }

        Op::Pattern(cp) => {
            let input = x.tree_id();
            let key = peel_filter(filter).id();
            let t = if cp.fallback {
                // More components than the NFA state mask can hold: match full paths.
                tree::remove_pred(
                    transaction,
                    &mut String::new(),
                    input,
                    &|path, isblob| {
                        isblob && cp.full.matches_with(path, tree::PATTERN_MATCH_OPTIONS)
                    },
                    key,
                )?
            } else {
                tree::remove_pattern(
                    transaction,
                    input,
                    cp,
                    key,
                    tree::CompiledPattern::initial_state(),
                )?
            };
            Ok(if t == input { x } else { x.with_tree(t) })
        }
        Op::Insert(dest_path, content) => {
            let (oid, mode, is_tree) = match content {
                InsertContent::Inline(s) => (
                    odb.write(gix_object::Kind::Blob, s.as_bytes()),
                    0o100644,
                    false,
                ),
                // The kind comes from the header alone; a missing oid folds into the
                // "neither" arm below.
                InsertContent::Oid(oid) => match odb.try_kind(*oid) {
                    Ok(Some(gix_object::Kind::Blob)) => (oid.to_owned(), 0o100644, false),
                    Ok(Some(gix_object::Kind::Tree)) => (oid.to_owned(), 0o040000, true),
                    _ => {
                        return Err(anyhow::anyhow!(
                            "insert: {} is neither a blob nor a tree",
                            oid
                        ));
                    }
                },
            };
            // `:$.=<oid>` replaces the whole output tree with the referenced object. Only a
            // tree is valid at the root; a blob there is rejected (it would otherwise be
            // silently dropped, since git trees cannot have a blob at their root).
            if dest_path.as_path() == std::path::Path::new(".") {
                if is_tree {
                    return Ok(x.with_tree(oid));
                }
                return Err(anyhow::anyhow!(
                    "insert: `:$.=<oid>` requires a tree; a blob cannot be placed at the tree root"
                ));
            }
            Ok(x.with_tree(tree::insert_oid(
                odb,
                tree::empty_id(),
                dest_path,
                oid,
                mode,
            )?))
        }
        Op::File(dest_path, source_path) => {
            // The source entry's raw mode is carried into the copy, like every other kept
            // entry; lookup failures fold into the fallback.
            let (file, mode) =
                match tree::get_path_entry(transaction, odb, x.tree_id(), source_path) {
                    Ok(Some(e)) => (e.oid, e.mode.value() as i32),
                    _ => (gix_hash::ObjectId::null(gix_hash::Kind::Sha1), 0o100644),
                };
            Ok(x.with_tree(tree::insert_oid(
                odb,
                tree::empty_id(),
                dest_path,
                file,
                mode,
            )?))
        }

        Op::Subdir(path) => {
            // A missing path, a non-tree entry (e.g. a blob), or a failed lookup has no
            // subtree.
            let subtree = match tree::get_path_entry(transaction, odb, x.tree_id(), path) {
                Ok(Some(entry)) if entry.mode.is_tree() => entry.oid,
                _ => tree::empty_id(),
            };
            Ok(x.with_tree(subtree))
        }
        Op::Prefix(path) => {
            let tree = x.tree_id();
            Ok(x.with_tree(tree::insert_oid(
                odb,
                tree::empty_id(),
                path,
                tree,
                0o040000,
            )?))
        }

        Op::Subtract(a, b) => {
            let af = apply_impl(transaction, odb, *a, x.clone())?;
            let bf = apply_impl(transaction, odb, *b, x.clone())?;
            let bu = apply_impl(transaction, odb, invert(*b)?, bf)?;
            let ba = apply_impl(transaction, odb, *a, bu)?.tree_id();
            Ok(x.with_tree(tree::subtract(transaction, af.tree_id(), ba)?))
        }
        Op::Exclude(b) => {
            let bf = apply_impl(transaction, odb, *b, x.clone())?.tree_id();
            let tree = x.tree_id();
            Ok(x.with_tree(tree::subtract(transaction, tree, bf)?))
        }
        Op::Select(b) => {
            let bf = apply_impl(transaction, odb, *b, x.clone())?.tree_id();
            let inside = tree::intersect(transaction, x.tree_id(), bf)?;
            Ok(x.with_tree(inside))
        }

        Op::Paths => {
            let tree = x.tree_id();
            Ok(x.with_tree(tree::pathstree("", tree, transaction)?))
        }
        Op::Index => {
            let index_cache = transaction.trigram_index_cache(x.commit);
            let tree = x.tree_id();
            Ok(x.with_tree(josh_search::trigram_index(
                odb,
                &index_cache,
                &mut transaction.trigram_indexer(),
                tree,
            )?))
        }

        Op::Invert => {
            let tree = x.tree_id();
            Ok(x.with_tree(tree::invert_paths(transaction, odb, "", tree)?))
        }

        Op::Workspace(path) => {
            // An unreadable or unparseable input tree is a hard error (the get_* helpers
            // below would fold it to Op::Empty), and every probe shares the one parse.
            let tree = x.tree_id();
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let f = get_workspace(transaction, odb, tree, &tree_reader, path);
            apply_impl(transaction, odb, f, x)
        }
        Op::Stored(path) => {
            let tree = x.tree_id();
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let f = get_stored(transaction, odb, tree, &tree_reader, path);
            apply_impl(transaction, odb, f, x)
        }
        Op::Starlark(path, subfilter) => {
            let tree = x.tree_id();
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let f = get_starlark(transaction, odb, tree, &tree_reader, path, *subfilter);
            apply_impl(transaction, odb, f, x)
        }
        Op::TreeId(path, subfilter) => {
            let applied = apply_impl(transaction, odb, *subfilter, x.clone())?;
            let oid_str = applied.tree_id().to_string();
            apply_impl(
                transaction,
                odb,
                to_filter(Op::Insert(path.clone(), InsertContent::Inline(oid_str))),
                x,
            )
        }
        Op::ObjectRef(path) => {
            if let Ok(Some(entry)) = tree::get_path_entry(transaction, odb, x.tree_id(), path) {
                let oid_str = entry.oid.to_string();
                let blob_oid = odb.write(gix_object::Kind::Blob, oid_str.as_bytes());
                Ok(x.with_tree(tree::insert_oid(
                    odb,
                    tree::empty_id(),
                    path,
                    blob_oid,
                    0o100644,
                )?))
            } else {
                Ok(x)
            }
        }
        Op::ObjectDeref(path) => {
            // If path doesn't exist in input, do nothing (nop).
            let entry = match tree::get_path_entry(transaction, odb, x.tree_id(), path) {
                Ok(Some(e)) => e,
                _ => return Ok(x),
            };
            // Path exists: read OID string from blob content.
            let oid_str = if let Some(blob) = tree::blob_bytes(odb, entry.oid) {
                std::str::from_utf8(&blob)?
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            } else {
                String::new()
            };
            if let Ok(oid) = gix_hash::ObjectId::from_str(&oid_str) {
                // Kind by header, never by `contains`: `read_header`'s disk fallback
                // virtualizes the empty tree, `exists` does not.
                let (oid, mode) = match odb.try_kind(oid) {
                    Ok(Some(gix_object::Kind::Tree)) => (oid, 0o040000),
                    Ok(Some(gix_object::Kind::Blob)) => (oid, 0o100644),
                    _ => {
                        return Err(anyhow::anyhow!(":#: object not found in repo: {}", oid));
                    }
                };
                Ok(x.with_tree(tree::insert_oid(odb, tree::empty_id(), path, oid, mode)?))
            } else {
                // Content is not a valid OID: insert empty blob at path.
                let empty_blob = odb.write(gix_object::Kind::Blob, b"");
                Ok(x.with_tree(tree::insert_oid(
                    odb,
                    tree::empty_id(),
                    path,
                    empty_blob,
                    0o100644,
                )?))
            }
        }

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| apply_impl(transaction, odb, *f, x.clone()))
                .collect::<anyhow::Result<_>>()?;
            let filtered: Vec<_> = filters
                .iter()
                .zip(filtered.iter().map(|t| t.tree_id()))
                .collect();
            Ok(x.with_tree(tree::compose(transaction, filtered)?))
        }

        Op::Chain(filters) => {
            let mut result = x;
            for filter in filters {
                result = apply_impl(transaction, odb, *filter, result)?;
            }
            Ok(result)
        }
        Op::Hook(_) => Err(anyhow!("not applicable to tree: hook")),

        Op::Embed(..) => Err(anyhow!("not applicable to tree: embed")),
        Op::Unapply(target, uf) => {
            check_experimental_features_enabled("unapply filter")?;
            if let LazyRef::Resolved(target) = target {
                let target = objects::CommitData::read(odb, target.to_owned())?;
                // The message must parse as an oid, so non-UTF-8 is an error.
                let target_msg = target.message()?;
                let target = gix_hash::ObjectId::from_str(std::str::from_utf8(target_msg)?)?;
                let target_tree = git::read_tree_id(odb, target)?;
                /* dbg!(&uf); */
                Ok(Rewrite::from_tree(filter::unapply(
                    transaction,
                    *uf,
                    x.tree_id(),
                    target_tree,
                    None,
                )?))
            } else {
                return Err(anyhow!("unresolved lazy ref"));
            }
        }
        Op::Pin(_) => Ok(x),
        Op::Downstack(_) => Err(anyhow!("not applicable to tree: downstack")),
        Op::Meta(_, _) => unreachable!(),
    }
}

/// Calculate a tree with minimal differences from `parent_tree`
/// such that `apply(unapply(tree, parent_tree)) == tree`.
///
/// `commits` carries the commit being reconstructed and the specific original parent whose tree is
/// `parent_tree`. It is required to reverse commit-resolved per-commit filters (`:rev` / `:hook`),
/// whose sub-filter is chosen from commit identity rather than the tree; callers that only ever pass
/// tree-reversible filters pass `None`.
pub fn unapply(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: gix_hash::ObjectId,
    parent_tree: gix_hash::ObjectId,
    commits: Option<(gix_hash::ObjectId, gix_hash::ObjectId)>,
) -> anyhow::Result<gix_hash::ObjectId> {
    // A `:rev(...)` filter has no static inverse (`invert` returns `Err`, like `:workspace` /
    // `:stored` / `:starlark`), so this generic path is automatically skipped for it and it falls
    // through to the per-commit handler / chain recursion below.
    if let Ok(inverted) = invert(filter) {
        let filtered = apply(
            transaction,
            invert(inverted)?,
            Rewrite::from_tree(parent_tree),
        )?;
        let matching = apply(transaction, inverted, filtered)?;
        let stripped = tree::subtract(transaction, parent_tree, matching.tree_id())?;
        let x = apply(transaction, inverted, Rewrite::from_tree(tree))?;

        return tree::overlay(transaction, x.tree_id(), stripped);
    }

    // `peel_op` (not `to_op`) so a `:~(...)[...]` meta wrapper around e.g. `:rev(...)` is seen
    // through: the meta options (history flags) only affect the forward direction, and the
    // per-commit reverse handler needs the wrapped operator.
    if let Some(ws) =
        unapply_per_rev_filter(transaction, &peel_op(filter), tree, parent_tree, commits)?
    {
        return Ok(ws);
    }

    if let Op::Chain(filters) = peel_op(filter) {
        // Split into first and rest, unapply recursively
        let (first, rest) = match filters.split_first() {
            Some((first, rest)) => (first, rest),
            None => return Ok(tree),
        };

        if rest.is_empty() {
            return unapply(transaction, *first, tree, parent_tree, commits);
        }

        let rest_chain = to_filter(Op::Chain(rest.to_vec()));

        // Compute filtered_parent_tree for the first filter. `:rev`/`:hook` select their sub-filter
        // from commit identity rather than the tree, so they cannot be applied at tree level:
        // resolve the parent's concrete sub-filter and apply that instead. Without commit context
        // (tree-only callers) it is unresolvable, so this is an error rather than a silent identity.
        let first_op = peel_op(*first);
        let filtered_parent_tree = if is_commit_resolved_filter(&first_op) {
            let (_, parent) = commits.ok_or_else(|| {
                anyhow!(
                    "cannot reverse a per-commit filter (`:rev`/`:hook`) without commit context"
                )
            })?;
            let parent_filter = resolve_commit_filter(transaction, &first_op, parent)?;
            apply(transaction, parent_filter, Rewrite::from_tree(parent_tree))?.tree_id()
        } else {
            let first_normalized = if let Ok(first_inverted) = invert(*first) {
                invert(first_inverted)?
            } else {
                *first
            };
            apply(
                transaction,
                first_normalized,
                Rewrite::from_tree(parent_tree),
            )?
            .tree_id()
        };

        // Recursively unapply: first unapply the rest, then unapply first
        return unapply(
            transaction,
            *first,
            unapply(transaction, rest_chain, tree, filtered_parent_tree, commits)?,
            parent_tree,
            commits,
        );
    }

    Err(anyhow!("filter cannot be unapplied"))
}

/// Reverse a per-commit filter (`:workspace` / `:stored` / `:starlark` / `:rev` / `:hook`), whose
/// concrete sub-filter differs between the commit being reconstructed and its parent. `filter` is
/// the sub-filter resolved for the commit, `original_filter` the one resolved for the specific
/// parent whose tree is `parent_tree`. The reconstruction reproduces the out-of-filter content of
/// `parent_tree` (strip) and overlays the reconstructed in-filter content of `tree`.
fn reverse_strip_overlay(
    transaction: &cache::Transaction,
    filter: Filter,
    original_filter: Filter,
    tree: gix_hash::ObjectId,
    parent_tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let filtered = apply(
        transaction,
        original_filter,
        Rewrite::from_tree(parent_tree),
    )?;
    let matching = apply(transaction, invert(original_filter)?, filtered)?;
    let stripped = tree::subtract(transaction, parent_tree, matching.tree_id())?;
    let x = apply(transaction, invert(filter)?, Rewrite::from_tree(tree))?;

    tree::overlay(transaction, x.tree_id(), stripped)
}

/// Resolve the concrete sub-filter of a per-commit filter whose selector lives in *commit identity*
/// rather than the tree: `:rev(...)` (commit ancestry) and `:hook` (hook lookup). These mirror the
/// forward `Op::Rev`/`Op::Hook` paths and, unlike `:workspace`/`:stored`/`:starlark`, cannot be
/// resolved from a tree. Returns `Err` for any other op (callers only reach it for rev/hook).
fn resolve_commit_filter(
    transaction: &cache::Transaction,
    op: &Op,
    commit: gix_hash::ObjectId,
) -> anyhow::Result<Filter> {
    match op {
        Op::Rev(revs) => get_rev_filter(transaction, commit, revs),
        Op::Hook(hook) => transaction.lookup_filter_hook(hook, commit),
        _ => Err(anyhow!("not a per-commit (rev/hook) filter")),
    }
}

/// Whether `op` is a per-commit filter whose selector lives in commit identity (`:rev` / `:hook`),
/// so reversing it requires commit context and it cannot be applied at tree level.
fn is_commit_resolved_filter(op: &Op) -> bool {
    matches!(op, Op::Rev(_) | Op::Hook(_))
}

fn unapply_per_rev_filter(
    transaction: &cache::Transaction,
    op: &Op,
    tree: gix_hash::ObjectId,
    parent_tree: gix_hash::ObjectId,
    commits: Option<(gix_hash::ObjectId, gix_hash::ObjectId)>,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    if is_commit_resolved_filter(op) {
        // `:rev`/`:hook` select their sub-filter from commit identity (mirroring the forward
        // `Op::Rev`/`Op::Hook` paths): resolve it for the commit being reconstructed and for the
        // specific parent whose tree is `parent_tree`. Unlike the tree-resolved per-commit filters
        // below, this cannot be read from a tree, so without commit context it is an error rather
        // than a silent identity.
        let (commit, parent) = commits.ok_or_else(|| {
            anyhow!("cannot reverse a per-commit filter (`:rev`/`:hook`) without commit context")
        })?;
        let filter = resolve_commit_filter(transaction, op, commit)?;
        let original_filter = resolve_commit_filter(transaction, op, parent)?;
        return Ok(Some(reverse_strip_overlay(
            transaction,
            filter,
            original_filter,
            tree,
            parent_tree,
        )?));
    }

    match op {
        Op::Workspace(path) => {
            let odb = transaction.odb();
            let tree = pre_process_tree(transaction, tree)?;
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let parent_reader = tree::read_tree(transaction, odb, parent_tree)?;
            let workspace = get_filter(
                transaction,
                odb,
                tree,
                &tree_reader,
                Path::new("workspace.josh"),
            );
            let original_workspace = get_filter(
                transaction,
                odb,
                parent_tree,
                &parent_reader,
                &path.join("workspace.josh"),
            );

            let root = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = to_filter(Op::File(
                Path::new("workspace.josh").to_owned(),
                Path::new("workspace.josh").to_owned(),
            ));
            let wsj_file = root.chain(wsj_file);
            let filter = compose(&[wsj_file, compose(&[workspace, root])]);
            let original_filter = compose(&[wsj_file, compose(&[original_workspace, root])]);
            Ok(Some(reverse_strip_overlay(
                transaction,
                filter,
                original_filter,
                tree,
                parent_tree,
            )?))
        }
        Op::Stored(path) => {
            let odb = transaction.odb();
            let stored_path = path.with_added_extension("josh");
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let parent_reader = tree::read_tree(transaction, odb, parent_tree)?;
            let stored = get_filter(transaction, odb, tree, &tree_reader, &stored_path);
            let original_stored =
                get_filter(transaction, odb, parent_tree, &parent_reader, &stored_path);

            let sj_file = Filter::new().file(stored_path.clone());
            let filter = compose(&[sj_file, stored]);
            let original_filter = compose(&[sj_file, original_stored]);
            Ok(Some(reverse_strip_overlay(
                transaction,
                filter,
                original_filter,
                tree,
                parent_tree,
            )?))
        }
        Op::Starlark(path, subfilter) => {
            let odb = transaction.odb();
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let parent_reader = tree::read_tree(transaction, odb, parent_tree)?;
            let filter = get_starlark(transaction, odb, tree, &tree_reader, path, *subfilter);
            let original_filter = get_starlark(
                transaction,
                odb,
                parent_tree,
                &parent_reader,
                path,
                *subfilter,
            );
            Ok(Some(reverse_strip_overlay(
                transaction,
                filter,
                original_filter,
                tree,
                parent_tree,
            )?))
        }
        _ => Ok(None),
    }
}

fn pre_process_tree(
    transaction: &cache::Transaction,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    let path = Path::new("workspace.josh");
    let ws_file = tree::get_blob(transaction, odb, tree, path);
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

    let tree = tree::insert_oid(
        odb,
        tree,
        path,
        odb.write(gix_object::Kind::Blob, blob.as_bytes()),
        0o100644, // Should this handle filemode?
    )?;

    Ok(tree)
}

/// Compute the warnings (filters not matching anything) for the filter applied to the tree
pub fn compute_warnings(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: gix_hash::ObjectId,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut filter = filter;

    let odb = transaction.odb();

    if let Op::Workspace(path) = to_op(filter) {
        let workspace_filter = &tree::get_blob(
            transaction,
            odb,
            tree,
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
        let stored_filter = &tree::get_blob(transaction, odb, tree, &stored_path);
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
            warnings.append(&mut compute_warnings2(transaction, f, tree));
        }
    } else {
        warnings.append(&mut compute_warnings2(transaction, filter, tree));
    }
    warnings
}

fn compute_warnings2(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: gix_hash::ObjectId,
) -> Vec<String> {
    let mut warnings = Vec::new();

    let x = apply(transaction, filter, Rewrite::from_tree(tree));
    if let Ok(x) = x
        && x.tree_id() == tree::empty_id()
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
    commit: gix_hash::ObjectId,
    tip: gix_hash::ObjectId,
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
                for parent in crate::git::read_parent_ids(transaction.odb(), commit)? {
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
        Op::Select(f) => to_filter(Op::Select(legalize_pin(f, c))),
        Op::Meta(meta, f) => to_filter(Op::Meta(meta, legalize_pin(f, c))),
        Op::Pin(f) => c(f),
        _ => f,
    }
}

fn needs_legalization(f: Filter) -> bool {
    match to_op(f) {
        Op::Stored(_) | Op::Starlark(_, _) => true,
        Op::Compose(filters) | Op::Chain(filters) => filters.iter().any(|&f| needs_legalization(f)),
        Op::Subtract(a, b) => needs_legalization(a) || needs_legalization(b),
        Op::Exclude(f) | Op::Select(f) | Op::Pin(f) | Op::TreeId(_, f) => needs_legalization(f),
        Op::Meta(_, f) => needs_legalization(f),
        _ => false,
    }
}

fn legalize_stored(
    t: &cache::Transaction,
    odb: &josh_memodb::Odb,
    f: Filter,
    tree: gix_hash::ObjectId,
    reader: &tree::TreeReader,
) -> anyhow::Result<Filter> {
    if !needs_legalization(f) {
        return Ok(f);
    }

    if let Some(f) = t.get_legalize((f, tree)) {
        return Ok(f);
    }

    // Put an entry into the hashtable to prevent infinite recursion.
    // If we get called with the same arguments again before we return,
    // Above check breaks the recursion.
    t.insert_legalize((f, tree), Filter::new().empty());

    let r = match to_op(f) {
        Op::Compose(f) => {
            let f = f
                .into_iter()
                .map(|f| legalize_stored(t, odb, f, tree, reader))
                .collect::<anyhow::Result<Vec<_>>>()?;
            to_filter(Op::Compose(f))
        }
        Op::Chain(filters) => {
            let mut result = Vec::with_capacity(filters.len());
            let mut current_tree = tree;
            for (i, filter) in filters.iter().enumerate() {
                // Chain steps past the first run against freshly applied intermediate
                // trees; those need their own read+parse (the caller's parse only covers
                // `tree`).
                let legalized = if current_tree == tree {
                    legalize_stored(t, odb, *filter, tree, reader)?
                } else {
                    let current_parsed = tree::read_tree(t, odb, current_tree)?;
                    legalize_stored(t, odb, *filter, current_tree, &current_parsed)?
                };
                result.push(legalized);
                // Only compute the intermediate tree if a subsequent element still needs
                // legalization — the tree is passed to legalize_stored, which uses it to
                // resolve Stored/Starlark refs.  Elements that don't need legalization
                // (e.g. a trailing Prefix) return immediately via the fast-path, so
                // running apply just to compute a tree they'll never use is pure waste.
                if filters[i + 1..].iter().any(|&f| needs_legalization(f)) {
                    current_tree = apply(t, legalized, Rewrite::from_tree(current_tree))?.tree;
                }
            }
            to_filter(Op::Chain(result))
        }
        Op::Subtract(a, b) => to_filter(Op::Subtract(
            legalize_stored(t, odb, a, tree, reader)?,
            legalize_stored(t, odb, b, tree, reader)?,
        )),
        Op::Exclude(f) => to_filter(Op::Exclude(legalize_stored(t, odb, f, tree, reader)?)),
        Op::Select(f) => to_filter(Op::Select(legalize_stored(t, odb, f, tree, reader)?)),
        Op::Meta(meta, f) => to_filter(Op::Meta(meta, legalize_stored(t, odb, f, tree, reader)?)),
        Op::Pin(f) => to_filter(Op::Pin(legalize_stored(t, odb, f, tree, reader)?)),
        Op::Stored(path) => get_stored(t, odb, tree, reader, &path),
        Op::Starlark(path, sub) => get_starlark(
            t,
            odb,
            tree,
            reader,
            &path,
            legalize_stored(t, odb, sub, tree, reader)?,
        ),
        Op::TreeId(path, f) => {
            to_filter(Op::TreeId(path, legalize_stored(t, odb, f, tree, reader)?))
        }
        _ => f,
    };

    t.insert_legalize((f, tree), r);

    Ok(r)
}

// Compute the difference between the current commit's filter and each parent's filter.
// Figure out which new entries in this commit's tree appeared because of the filter changes,
// as opposed of the changes of the trees itself. For those entries, we create synthetic
// merges that splice in the history of the new entries coming in. Returns `Ok(None)` to
// propagate the same short-circuit as `apply_to_commit2` (parent not yet filtered).
fn compute_splice_parents(
    transaction: &cache::Transaction,
    commit_filter: Filter,
    parent_filters: Vec<(gix_hash::ObjectId, Filter)>,
    meta: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<Option<Vec<gix_hash::ObjectId>>> {
    // This is only reached when history splicing is enabled, and `per_rev_filter` rejects `:pin`
    // in that case, so the filters here are pin-free -- no pin handling is needed.
    let splice_parents = parent_filters
        .into_iter()
        .map(|(parent, pcw)| {
            // Collapsing this Subtract as far as possible really matters here: it folds to
            // `Empty` whenever the new filter selects no paths beyond the parent's -- not only
            // for equivalent filters but for any change that does not expand the set, such as a
            // rename or a removal -- and an empty result lets `apply_to_commit2` skip a whole
            // history walk. `optimize` skips the trailing-Compose distribution, which can leave
            // the subtract less collapsed, so use `minimize` here to flatten fully and give
            // `step`'s set-difference the flat form it needs to cancel.
            let f = opt::minimize(to_filter(Op::Subtract(commit_filter.peel(), pcw.peel())));
            let f = propagate_meta(f, meta);
            apply_to_commit2(f, parent, transaction)
        })
        .collect::<anyhow::Result<Option<Vec<_>>>>()?;

    let splice_parents = some_or!(splice_parents, { return Ok(None) });

    Ok(Some(
        splice_parents
            .into_iter()
            .filter(|&oid| oid != gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
            .collect(),
    ))
}

fn per_rev_filter(
    transaction: &cache::Transaction,
    commit: &objects::CommitData,
    filter: Filter,
    commit_filter: Filter,
    parent_filters: Vec<(gix_hash::ObjectId, Filter)>,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    // Propagate any meta-options from the outer filter (e.g. :~(gpgsig="norm-lf")[:rev(...)])
    // into the per-commit filter so they are applied during commit rewriting.
    let meta = filter.into_meta();
    let commit_filter = propagate_meta(commit_filter, &meta);

    // Legalize the pins two ways up front (selected, or excluded); they differ iff the filter
    // uses `:pin`. `Select` and `Exclude` are exact complements over the tree at the pin point,
    // so the pinned region stays well-defined even when the pin argument is a generative filter
    // (one whose output is not an in-place subset of its input). Splicing is on unless the
    // "no-splice" history flag is set, and combining it with `:pin` has no well-defined
    // semantics, so reject that rather than guess.
    let legalized_a = legalize_pin(commit_filter.peel(), &|f| to_filter(Op::Select(f)));
    let legalized_b = legalize_pin(commit_filter.peel(), &|f| to_filter(Op::Exclude(f)));
    let uses_pin = legalized_a != legalized_b;
    let no_splice = history::history_flag(meta.get("history").map(String::as_str), "no-splice");
    if uses_pin && !no_splice {
        return Err(anyhow!(
            "combining :pin with history splicing is not supported; \
             set the \"no-splice\" history flag"
        ));
    }

    let splice_parents = if no_splice {
        vec![]
    } else {
        some_or!(
            compute_splice_parents(transaction, commit_filter, parent_filters, &meta)?,
            { return Ok(None) }
        )
    };

    let normal_parents = commit
        .parent_ids()
        .map(|parent| transaction.get(filter, parent))
        .collect::<anyhow::Result<Option<Vec<gix_hash::ObjectId>>>>()?;
    let normal_parents = some_or!(normal_parents, { return Ok(None) });

    // Special case: `:pin` filter needs to be aware of filtered history
    let pin_details = if let Some(&parent) = normal_parents.first() {
        if uses_pin {
            let pin_subtract = apply(
                transaction,
                opt::optimize(to_filter(Op::Subtract(legalized_a, legalized_b))),
                Rewrite::from_commit_data(commit)?,
            )?;

            let parent_tree_id = history::filtered_parent_tree_id(transaction, parent)?;

            // Re-content the pinned path set from the parent's filtered tree: keep exactly the
            // parent's entries whose path is pinned. Equivalent to the older
            // `populate(pathstree(pin_subtract), parent)` spelling of "select the pinned paths out
            // of the previous tree", but in a single path-intersection pass.
            let pin_overlay = tree::intersect(transaction, parent_tree_id, pin_subtract.tree_id())?;

            Some((pin_subtract.tree_id(), pin_overlay))
        } else {
            None
        }
    } else {
        None
    };

    let filtered_parent_ids: Vec<_> = normal_parents.into_iter().chain(splice_parents).collect();

    if let Op::Squash(None) = to_op(commit_filter.peel()) {
        // `:SQUASH` as a per-commit filter squashes the commit away, mapping it
        // to the surviving filtered parent with the largest sequence number.
        // Ancestors have strictly smaller sequence numbers, so a parent that
        // has all others as ancestors always wins and nothing gets
        // disconnected; a fixed parent position would instead strand lineages
        // that only join through other parents (subtree sync merges carry the
        // newer subtree tip as their second parent).
        let mut target = None;
        for id in filtered_parent_ids
            .iter()
            .filter(|x| **x != gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
        {
            let seq = cache::compute_sequence_number(transaction, *id)?;
            if target.map(|(top, _)| seq > top).unwrap_or(true) {
                target = Some((seq, *id));
            }
        }
        // Squashing a merge is only lossless when the chosen parent has every
        // other surviving parent in its ancestry; incomparable parents mean
        // two preserved lineages join only inside the squashed region and one
        // of them would silently become unreachable. Fail instead.
        if let Some((_, target_id)) = target {
            for id in filtered_parent_ids.iter().filter(|x| {
                **x != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) && **x != target_id
            }) {
                if !objects::is_descendant_of(transaction.odb(), target_id, *id)? {
                    return Err(anyhow!(
                        "cannot squash {}: its filtered parents {} and {} do not descend \
                         from one another. `:SQUASH` can only preserve a single lineage",
                        commit.id(),
                        target_id,
                        id
                    ));
                }
            }
        }
        return Ok(Some(history::drop_commit(
            commit.id(),
            target.map(|(_, id)| id).into_iter().collect(),
            transaction,
            filter,
        )?));
    }

    let mut tree_data = apply(
        transaction,
        commit_filter,
        Rewrite::from_commit_data(commit)?,
    )?;

    if let Some((pin_subtract, pin_overlay)) = pin_details {
        let with_exclude = tree::subtract(transaction, tree_data.tree_id(), pin_subtract)?;
        let with_overlay = tree::overlay(transaction, pin_overlay, with_exclude)?;

        tree_data = tree_data.with_tree(with_overlay);
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

/// Rebuild the stack from `base_oid` to `change_oid`, dropping intermediate commits
/// whose changed paths are disjoint from the tip's changes, then reapply the tip
/// onto the minimised base via a 3-way merge. Returns the new tip OID.
pub fn downstack(
    transaction: &cache::Transaction,
    change_oid: gix_hash::ObjectId,
    base_oid: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if !objects::is_descendant_of(transaction.odb(), change_oid, base_oid)? {
        return Err(anyhow!(
            "change {} is not a descendant of base {}",
            change_oid,
            base_oid
        ));
    }

    // Collect commits from base to change (exclusive of base, inclusive of
    // change). On the first-parent spine there is exactly one path, so
    // pruning at the base is the same as excluding its history -- no
    // RangeWalk needed. A base that passes the descendant check but is NOT on
    // the spine would make "the stack from base" ill-defined; error instead
    // of answering.
    let odb = transaction.odb();
    let mut walk = objects::RevWalk::new(odb);
    walk.simplify_first_parent();
    walk.push(change_oid)?;
    let mut seen_base = false;
    // Reverse-topo order: oldest first, `change` last.
    let mut oids = walk.into_topo_vec(|oid| {
        if oid == base_oid {
            seen_base = true;
        }
        oid == base_oid
    })?;
    if !seen_base {
        return Err(anyhow!(
            "base {} is not on the first-parent chain of {}",
            base_oid,
            change_oid
        ));
    }
    oids.reverse();

    if oids.is_empty() {
        return Ok(change_oid);
    }

    let mut commits: Vec<objects::CommitData> = oids
        .into_iter()
        .map(|oid| objects::CommitData::read(odb, oid))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // The last commit is `change`; split it off from the intermediates
    let change_commit = commits.pop().unwrap();

    // Seed the affected dependency set with the change's own deps, then walk
    // intermediates backwards, keeping any commit whose deps intersect.
    let mut affected = downstack_commit_deps(transaction, odb, &change_commit)?;
    let mut needed: Vec<bool> = vec![false; commits.len()];
    for (i, intermediate) in commits.iter().enumerate().rev() {
        let deps = downstack_commit_deps(transaction, odb, intermediate)?;
        if !deps.is_disjoint(&affected) {
            needed[i] = true;
            affected.extend(deps);
        }
    }

    // Rebase needed intermediates forward onto current_base. The rebuilt base's tree is the
    // merge result just written, so no commit read-back is needed between steps.
    let mut current_base_id = base_oid;
    let mut current_base_tree = git::read_tree_id(odb, base_oid)?;
    for (intermediate, is_needed) in commits.iter().zip(needed.iter()) {
        if !is_needed {
            continue;
        }
        let inter_parent = intermediate.first_parent_id().ok_or_else(|| {
            anyhow!(
                "downstack: intermediate {} has no parent",
                intermediate.id()
            )
        })?;
        let new_tree_oid = cached_merge_trees(
            transaction,
            git::read_tree_id(odb, inter_parent)?,
            current_base_tree,
            intermediate.tree_id()?,
        )?;
        let new_oid = history::rewrite_commit(
            odb,
            intermediate,
            &[current_base_id],
            Rewrite::from_tree(new_tree_oid),
            history::GpgsigMode::Preserve,
        )?;
        current_base_id = new_oid;
        current_base_tree = new_tree_oid;
    }

    // Apply the change on top of the minimal base via 3-way merge.
    let change_parent = change_commit
        .first_parent_id()
        .ok_or_else(|| anyhow!("downstack: change {} has no parent", change_commit.id()))?;
    let new_tree_oid = cached_merge_trees(
        transaction,
        git::read_tree_id(odb, change_parent)?,
        current_base_tree,
        change_commit.tree_id()?,
    )?;

    // First-parent is remapped onto the minimal base; any further parents are
    // carried through untouched. A merge commit in the stack is treated as an
    // ordinary linear change (diffed against its first parent), but the rebuilt
    // change keeps its second-parent link so the merge survives the rewrite.
    let mut new_parents = vec![current_base_id];
    new_parents.extend(change_commit.parent_ids().skip(1));

    let new_oid = history::rewrite_commit(
        odb,
        &change_commit,
        &new_parents,
        Rewrite::from_tree(new_tree_oid),
        history::GpgsigMode::Preserve,
    )?;

    Ok(new_oid)
}

/// Memo git2's 3-way `merge_trees` + `write_tree_to` pair within the
/// active transaction. The merged tree oid is a pure function of the three
/// input tree oids (with `MergeOptions::None`), and `split_changes` issues
/// many identical triples across per-tip `downstack` calls; turning each
/// repeat into a hashmap hit is the win. Scoping to the transaction (rather
/// than process-wide) keeps `josh-proxy` from accumulating entries across
/// unrelated operations.
fn cached_merge_trees(
    transaction: &cache::Transaction,
    a: gix_hash::ObjectId,
    b: gix_hash::ObjectId,
    c: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    // Conventional 3-way identities. When the ancestor matches one side, the
    // merge result is the other side -- common in a clean linear stack where
    // the rebuilt parent's tree already equals the original intermediate's
    // parent tree -- so we skip both the lookup and the merge.
    if a == b {
        return Ok(c);
    }
    if a == c {
        return Ok(b);
    }

    let key = (a, b, c);
    if let Some(hit) = transaction.get_merge_trees(key) {
        return Ok(hit);
    }
    let oid = objects::merge_trees(transaction.odb(), a, b, c)?;
    transaction.insert_merge_trees(key, oid);
    Ok(oid)
}

/// A dependency token used by `downstack` to decide whether two commits
/// touch the same surface area. `Path` comes from a commit's diff; `Series`
/// comes from a `Change-Series:` trailer and lets unrelated paths be linked
/// explicitly.
#[derive(Clone, Eq, Hash, PartialEq)]
pub(crate) enum DownstackDep {
    Path(String),
    Series(String),
}

/// Cache deps per commit oid within a single transaction. `split_changes`
/// asks for the same intermediate's deps O(N) times across the stack, and a
/// commit's deps set is a pure function of its oid (parent tree + own tree +
/// trailer block) -- so memoising it collapses the duplicated work. The map
/// lives on the transaction so `josh-proxy` doesn't accumulate entries
/// across unrelated operations.
fn downstack_commit_deps(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    commit: &objects::CommitData,
) -> anyhow::Result<std::collections::HashSet<DownstackDep>> {
    let oid = commit.id();
    if let Some(hit) = transaction.get_downstack_deps(oid) {
        return Ok(hit);
    }

    // Use the in-crate `tree::diff_paths` rather than git2's full diff
    // pipeline: it short-circuits subtrees by oid equality, which collapses
    // the typical "stack of single-file commits" deps walk down to a few
    // tree lookups per call. An unreadable first parent is a hard error;
    // only the no-parent case diffs against nothing.
    let parent_tree_id = commit
        .first_parent_id()
        .map(|p| git::read_tree_id(odb, p))
        .transpose()?
        .unwrap_or(gix_hash::ObjectId::null(gix_hash::Kind::Sha1));
    let commit_tree_id = commit.tree_id()?;

    let mut deps = std::collections::HashSet::new();
    for (path, _polarity) in tree::diff_paths(odb, parent_tree_id, commit_tree_id, "")? {
        deps.insert(DownstackDep::Path(path));
    }

    // A non-UTF-8 message reads as empty.
    let message = commit
        .message()
        .ok()
        .and_then(|m| std::str::from_utf8(m).ok())
        .unwrap_or("");
    let (_, series) = crate::trailers::parse_change_meta(message);
    for s in series {
        deps.insert(DownstackDep::Series(s));
    }

    transaction.insert_downstack_deps(oid, deps.clone());
    Ok(deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn empty_tree(repo: &gix::Repository) -> gix_hash::ObjectId {
        gix_object::Write::write(
            &repo.objects,
            &gix_object::Tree {
                entries: Vec::new(),
            },
        )
        .unwrap()
    }

    fn build_tree(repo: &gix::Repository, files: &[(&str, &str)]) -> gix_hash::ObjectId {
        let mut builder = repo.edit_tree(empty_tree(repo)).unwrap();
        for (path, content) in files {
            let oid = josh_gix_ext::write_blob(&repo.objects, content.as_bytes()).unwrap();
            builder
                .upsert(*path, gix::objs::tree::EntryKind::Blob, oid)
                .unwrap();
        }
        builder.write().unwrap().detach()
    }

    fn write_commit(
        repo: &gix::Repository,
        tree: gix_hash::ObjectId,
        parents: &[gix_hash::ObjectId],
        message: &str,
    ) -> gix_hash::ObjectId {
        let signature = gix_actor::Signature {
            name: "t".into(),
            email: "t@e".into(),
            time: gix_actor::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        josh_gix_ext::write_commit(
            &repo.objects,
            tree,
            parents,
            &signature,
            &signature,
            message,
        )
        .unwrap()
    }

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
        use josh_filter::persist::{as_tree, from_tree};

        let test_dir = tempfile::tempdir().unwrap();
        let repo = gix::init(test_dir.path()).unwrap();

        // Create a metadata filter
        let mut meta = std::collections::BTreeMap::new();
        meta.insert("key1".to_string(), "value1".to_string());
        meta.insert("key2".to_string(), "value2".to_string());
        let inner_filter = parse(":/sub1").unwrap();
        let meta_filter = to_filter(Op::Meta(meta.clone(), inner_filter));

        // Serialize to tree
        let tree_oid = as_tree(&repo.objects, meta_filter).unwrap();

        // Deserialize from tree
        let deserialized_filter = from_tree(&repo.objects, tree_oid).unwrap();

        // Verify the specs match
        let original_spec = spec(meta_filter);
        let deserialized_spec = spec(deserialized_filter);
        assert_eq!(
            original_spec, deserialized_spec,
            "Tree round-trip failed:\n  Original spec: {}\n  Deserialized spec: {}",
            original_spec, deserialized_spec
        );
    }

    // Apply-based correctness oracle for the optimizer's Subtract handling: applying the
    // optimized filter to a tree must produce the same tree as applying the unoptimized filter
    // (`apply` is the ground-truth interpreter), and optimizing an invertible filter must keep it
    // invertible (the workspace machinery inverts optimized filters).
    #[test]
    fn subtract_optimizer_apply_oracle() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let mut files = [
            "d1/s1/f0",
            "d1/s1/f1",
            "d2/s2/f0",
            "d2/s2/f1",
            "d3/s3/f0",
            "sub1/file1",
            "sub1/file2",
            "sub2/subsub/file2",
        ]
        .map(|path| (path, path))
        .to_vec();
        let ws_content = ":/sub1::file1\n:/sub1::file2\n::sub2/subsub/\n";
        files.extend([
            ("ws/workspace.josh", ws_content),
            ("ws2/workspace.josh", ws_content),
        ]);
        let tree = build_tree(&repo, &files);

        let cachestack = std::sync::Arc::new(
            cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path())),
        );
        let ctx = cache::TransactionContext::new(td.path(), cachestack);
        let t = ctx.open().unwrap();

        let apply_tree = |f: Filter| -> anyhow::Result<gix_hash::ObjectId> {
            Ok(apply(&t, f, Rewrite::from_tree(tree))?.tree_id())
        };

        // The pin-legalisation subtract, built exactly like per_rev_filter.
        let files: Vec<Filter> = ["d1/s1/f0", "d1/s1/f1", "d2/s2/f0", "d2/s2/f1", "d3/s3/f0"]
            .iter()
            .map(|p| Filter::new().file(p))
            .collect();
        let wide = josh_filter::compose(&files);
        let pins = josh_filter::compose(&[Filter::new().file("d1/s1/f0")]);
        let cf = wide.pin(pins);
        let pin_sub = to_filter(Op::Subtract(
            legalize_pin(cf.peel(), &|f| to_filter(Op::Select(f))),
            legalize_pin(cf.peel(), &|f| to_filter(Op::Exclude(f))),
        ));

        let mut cases = vec![pin_sub];
        for s in [
            ":workspace=ws",
            ":workspace=ws2",
            ":subtract[:/sub1,::ws/]",
            ":subtract[:[::sub1/,::sub2/],::sub1/]",
            ":subtract[:/d1,:/d1/s1]",
            ":subtract[:[a=:/sub1,b=:/sub2],:[a=:/sub1]]",
        ] {
            cases.push(parse(s).unwrap());
        }
        // The nested subtract the workspace history machinery builds and then inverts.
        let contents = parse(":[:/sub1::file1,:/sub1::file2,::sub2/subsub/]").unwrap();
        let wsj = parse(":/ws::workspace.josh").unwrap();
        cases.push(to_filter(Op::Subtract(
            to_filter(Op::Subtract(contents, wsj)),
            to_filter(Op::Subdir(std::path::PathBuf::from("ws"))),
        )));

        // A generative Insert ignores its input, so a chain that becomes empty before it must not
        // collapse to `Empty`. These filters have to be built as raw ASTs: `parse` optimizes, which
        // would apply (and previously mis-apply) the very rule under test.
        let ins = |p: &str, c: &str| {
            to_filter(Op::Insert(
                std::path::PathBuf::from(p),
                InsertContent::Inline(c.to_string()),
            ))
        };
        // Chain[Empty, Insert] -- the interior emptiness comes from an explicit `Empty`.
        cases.push(to_filter(Op::Chain(vec![
            to_filter(Op::Empty),
            ins("x", "y"),
        ])));
        // Chain[Prefix(p), Subdir(q), Insert] -- the interior emptiness comes from the
        // prefix/subdir-mismatch rule (`:prefix=p:/q` is always empty).
        cases.push(to_filter(Op::Chain(vec![
            to_filter(Op::Prefix(std::path::PathBuf::from("p"))),
            to_filter(Op::Subdir(std::path::PathBuf::from("q"))),
            ins("x", "y"),
        ])));

        // Chain[Compose[Subdir(d1), Subdir(d2)], Exclude(File(s2/f0, s1/f0))]. `File` reads its
        // source (`s1/f0`, present only under d1) from the *composed* tree, so distributing the
        // trailing `Exclude(File)` into each branch changes the result: sequentially the merged
        // tree contains `s1/f0`, so `File` recreates `s2/f0` and `Exclude` drops it; distributed,
        // the d2 branch has no `s1/f0`, so nothing is excluded and `s2/f0` survives.
        cases.push(to_filter(Op::Chain(vec![
            to_filter(Op::Compose(vec![
                to_filter(Op::Subdir(std::path::PathBuf::from("d1"))),
                to_filter(Op::Subdir(std::path::PathBuf::from("d2"))),
            ])),
            to_filter(Op::Exclude(to_filter(Op::File(
                std::path::PathBuf::from("s2/f0"),
                std::path::PathBuf::from("s1/f0"),
            )))),
        ])));

        for sub in cases {
            // Both `optimize` and `minimize` run the rules under test, so exercise both.
            for (name, optimized) in [
                ("optimize", opt::optimize(sub)),
                ("minimize", opt::minimize(sub)),
            ] {
                match (apply_tree(sub), apply_tree(optimized)) {
                    (Ok(g), Ok(o)) => {
                        assert_eq!(g, o, "{name} changed result for {sub:?} -> {optimized:?}")
                    }
                    (Err(ge), _) => panic!("ground-truth apply failed for {sub:?}: {ge}"),
                    (_, Err(oe)) => {
                        panic!("{name} apply failed for {sub:?} -> {optimized:?}: {oe}")
                    }
                }
                if invert(sub).is_ok() {
                    assert!(
                        invert(optimized).is_ok(),
                        "{name} broke invertibility of {sub:?} -> {optimized:?}"
                    );
                }
            }
        }
    }

    // `downstack` minimizes a change against a base by reparenting it onto a
    // rebuilt minimal base. A merge commit in the stack is treated as an
    // ordinary linear change (diffed against its first parent), but the rewrite
    // must carry its remaining parents through so the merge survives. Non-merge
    // changes stay single-parent.
    #[test]
    fn downstack_keeps_merge_second_parent() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let mk_tree = |files: &[(&str, &str)]| build_tree(&repo, files);
        let mk_commit = |tree: gix_hash::ObjectId, parents: &[gix_hash::ObjectId], msg: &str| {
            write_commit(&repo, tree, parents, msg)
        };

        // base <- side, and a merge of the two is the change being published.
        let base = mk_commit(mk_tree(&[("a", "1")]), &[], "base");
        let side = mk_commit(mk_tree(&[("a", "1"), ("s", "s")]), &[base], "side work");
        let merge = mk_commit(
            mk_tree(&[("a", "1"), ("m", "m")]),
            &[base, side],
            "merge change",
        );

        let cachestack = std::sync::Arc::new(
            cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path())),
        );
        let ctx = cache::TransactionContext::new(td.path(), cachestack);
        let t = ctx.open().unwrap();

        let out =
            objects::CommitData::read(&repo.objects, downstack(&t, merge, base).unwrap()).unwrap();
        let parents = out.parent_ids().collect::<Vec<_>>();
        assert_eq!(parents.len(), 2, "merge change lost its second parent");
        assert_eq!(parents[0], base, "first parent should be the minimal base");
        assert_eq!(parents[1], side, "second parent should be preserved");

        // A single-parent change is unaffected by the parent-preserving rewrite.
        let linear = mk_commit(mk_tree(&[("a", "1"), ("n", "n")]), &[base], "linear change");
        let out_linear =
            objects::CommitData::read(&repo.objects, downstack(&t, linear, base).unwrap()).unwrap();
        assert_eq!(out_linear.parent_count(), 1);
    }
    // josh-changes builds changes-ref writes on `unapply` with a
    // `subdir(p).prefix(p)` filter (self-inverse: "exclude previous value under
    // p, overlay the new view"). These cover the edges the push path never
    // hits: an absent base ref, and deletion via an empty view.
    #[test]
    fn unapply_subdir_prefix_leaf_write_and_delete() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let cachestack = std::sync::Arc::new(
            cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path())),
        );
        let ctx = cache::TransactionContext::new(td.path(), cachestack);
        let t = ctx.open().unwrap();

        let f = Filter::new().subdir("a/b").prefix("a/b");
        // The filter must be invertible for the generic unapply path.
        assert!(invert(f).is_ok());

        // Result trees live in the transaction's odb, so assert through
        // get_path_entry rather than walking the tree.
        let blob_id = |content: &str| {
            josh_gix_ext::write_blob(&repo.objects, content.as_bytes())
                .unwrap()
                .to_string()
        };
        let entry_id = |tree: gix_hash::ObjectId, path: &str| -> Option<String> {
            tree::get_path_entry(&t, t.odb(), tree, Path::new(path))
                .unwrap()
                .map(|e| e.oid.to_string())
        };

        // Leaf write onto an absent base (ref does not exist yet): the result
        // is the view itself.
        let view = build_tree(&repo, &[("a/b/c.txt", "new")]);
        let merged = unapply(&t, f, view, tree::empty_id(), None).unwrap();
        assert_eq!(merged, view);

        // Leaf write over an existing base: content under a/b is replaced,
        // everything else is preserved.
        let base = build_tree(&repo, &[("x.txt", "keep"), ("a/b/old.txt", "old")]);
        let merged = unapply(&t, f, view, base, None).unwrap();
        assert_eq!(entry_id(merged, "x.txt"), Some(blob_id("keep")));
        assert_eq!(entry_id(merged, "a/b/c.txt"), Some(blob_id("new")));
        assert_eq!(entry_id(merged, "a/b/old.txt"), None);
        // Leaf write where the base exists but the selected path does not
        // (the common new-change case): the view is merged in, the rest kept.
        let base_no_ns = build_tree(&repo, &[("x.txt", "keep")]);
        let merged = unapply(&t, f, view, base_no_ns, None).unwrap();
        assert_eq!(entry_id(merged, "x.txt"), Some(blob_id("keep")));
        assert_eq!(entry_id(merged, "a/b/c.txt"), Some(blob_id("new")));
        // Round-trip: applying the filter to the merged tree yields the view.
        let roundtrip = apply(&t, f, Rewrite::from_tree(merged)).unwrap();
        assert_eq!(roundtrip.tree_id(), view);

        // Empty view deletes the selected path only.
        let deleted = unapply(&t, f, tree::empty_id(), base, None).unwrap();
        assert_eq!(entry_id(deleted, "x.txt"), Some(blob_id("keep")));
        assert_eq!(entry_id(deleted, "a/b"), None);

        // The write shape store.rs uses for blob leaves: filter on the parent
        // directory (`subdir` on the leaf itself yields an empty tree for
        // blobs), insert the leaf into the filtered subset, merge back.
        let subset = apply(&t, f, Rewrite::from_tree(base)).unwrap().tree_id();
        let view = tree::insert_oid(
            t.odb(),
            subset,
            Path::new("a/b/c.txt"),
            josh_gix_ext::write_blob(&repo.objects, b"new").unwrap(),
            0o0100644,
        )
        .unwrap();
        let merged = unapply(&t, f, view, base, None).unwrap();
        assert_eq!(entry_id(merged, "x.txt"), Some(blob_id("keep")));
        assert_eq!(entry_id(merged, "a/b/old.txt"), Some(blob_id("old")));
        assert_eq!(entry_id(merged, "a/b/c.txt"), Some(blob_id("new")));

        // Blob-leaf deletion in the same shape: zero-oid insert into the
        // subset removes the entry.
        let view = tree::insert_oid(
            t.odb(),
            subset,
            Path::new("a/b/old.txt"),
            gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            0,
        )
        .unwrap();
        let deleted = unapply(&t, f, view, base, None).unwrap();
        assert_eq!(entry_id(deleted, "x.txt"), Some(blob_id("keep")));
        assert_eq!(entry_id(deleted, "a/b/old.txt"), None);
    }

    // Same contract for a deeper, multi-component path (`outbox/votes`-style):
    // subdir/prefix accept slashes, and so does their inverse.
    #[test]
    fn unapply_multi_component_path() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let cachestack = std::sync::Arc::new(
            cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path())),
        );
        let ctx = cache::TransactionContext::new(td.path(), cachestack);
        let t = ctx.open().unwrap();

        let f = Filter::new().subdir("ns/sub/leaf").prefix("ns/sub/leaf");
        assert!(invert(f).is_ok());

        let base = build_tree(
            &repo,
            &[("ns/sub/leaf/old", "old"), ("ns/keep/f.txt", "keep")],
        );
        let view = build_tree(&repo, &[("ns/sub/leaf/new", "new")]);
        let merged = unapply(&t, f, view, base, None).unwrap();

        let roundtrip = apply(&t, f, Rewrite::from_tree(merged)).unwrap();
        assert_eq!(roundtrip.tree_id(), view);

        let sibling = apply(
            &t,
            Filter::new().subdir("ns/keep").prefix("ns/keep"),
            Rewrite::from_tree(merged),
        )
        .unwrap();
        assert_eq!(
            sibling.tree_id(),
            build_tree(&repo, &[("ns/keep/f.txt", "keep")]),
            "sibling subtree must survive"
        );
    }
}
