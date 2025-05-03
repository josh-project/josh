use super::*;
use history::RewriteData;
use pest::Parser;
use std::path::Path;
mod opt;
mod parse;
pub mod tree;

pub use opt::invert;
pub use parse::get_comments;
pub use parse::parse;

lazy_static! {
    static ref FILTERS: std::sync::Mutex<std::collections::HashMap<Filter, Op>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref WORKSPACES: std::sync::Mutex<std::collections::HashMap<git2::Oid, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref ANCESTORS: std::sync::Mutex<std::collections::HashMap<git2::Oid, std::collections::HashSet<git2::Oid>>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

/// Filters are represented as `git2::Oid`, however they are not ever stored
/// inside the repo.
#[derive(
    Clone, Hash, PartialEq, Eq, Copy, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct Filter(git2::Oid);

impl std::convert::TryFrom<String> for Filter {
    type Error = JoshError;
    fn try_from(s: String) -> JoshResult<Filter> {
        parse(&s)
    }
}

impl From<Filter> for String {
    fn from(val: Filter) -> Self {
        spec(val)
    }
}

impl Default for Filter {
    fn default() -> Filter {
        nop()
    }
}

impl Filter {
    pub fn id(&self) -> git2::Oid {
        self.0
    }
}

impl std::fmt::Debug for Filter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        to_op(*self).fmt(f)
    }
}

pub fn nop() -> Filter {
    to_filter(Op::Nop)
}

pub fn empty() -> Filter {
    to_filter(Op::Empty)
}

pub fn message(m: &str) -> Filter {
    to_filter(Op::Message(m.to_string()))
}

pub fn squash(ids: Option<&[(git2::Oid, Filter)]>) -> Filter {
    if let Some(ids) = ids {
        to_filter(Op::Squash(Some(
            ids.iter()
                .map(|(x, y)| (LazyRef::Resolved(*x), y.clone()))
                .collect(),
        )))
    } else {
        to_filter(Op::Squash(None))
    }
}

fn to_filter(op: Op) -> Filter {
    let s = format!("{:?}", op);
    let f = Filter(
        git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).expect("hash_object filter"),
    );
    FILTERS.lock().unwrap().insert(f, op);
    f
}

fn to_op(filter: Filter) -> Op {
    FILTERS
        .lock()
        .unwrap()
        .get(&filter)
        .expect("unknown filter")
        .clone()
}

#[derive(Hash, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
enum LazyRef {
    Resolved(git2::Oid),
    Lazy(String),
}

impl LazyRef {
    fn to_string(&self) -> String {
        match self {
            LazyRef::Resolved(id) => format!("{}", id),
            LazyRef::Lazy(lazy) => format!("\"{}\"", lazy),
        }
    }
    fn parse(s: &str) -> JoshResult<LazyRef> {
        let s = s.replace("'", "\"");
        if let Ok(serde_json::Value::String(s)) = serde_json::from_str(&s) {
            return Ok(LazyRef::Lazy(s));
        }
        if let Ok(oid) = git2::Oid::from_str(&s) {
            return Ok(LazyRef::Resolved(oid));
        } else {
            return Err(josh_error(&format!("invalid ref: {:?}", s)));
        }
    }
}

#[derive(Clone, Debug)]
enum Op {
    Nop,
    Empty,
    Fold,
    Paths,

    // We use BTreeMap rather than HashMap to guarantee deterministic results when
    // converting to Filter
    Squash(Option<std::collections::BTreeMap<LazyRef, Filter>>),
    Author(String, String),
    Committer(String, String),

    // We use BTreeMap rather than HashMap to guarantee deterministic results when
    // converting to Filter
    Rev(std::collections::BTreeMap<LazyRef, Filter>),
    Join(std::collections::BTreeMap<LazyRef, Filter>),
    Linear,
    Unsign,

    RegexReplace(Vec<(regex::Regex, String)>),

    Index,
    Invert,

    File(std::path::PathBuf),
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),

    Glob(String),
    Message(String),

    Compose(Vec<Filter>),
    Chain(Filter, Filter),
    Subtract(Filter, Filter),
    Exclude(Filter),
}

/// Pretty print the filter on multiple lines with initial indentation level.
/// Nested filters will be indented with additional 4 spaces per nesting level.
pub fn pretty(filter: Filter, indent: usize) -> String {
    let filter = opt::simplify(filter);

    if let Op::Compose(filters) = to_op(filter) {
        if indent == 0 {
            let i = format!("\n{}", " ".repeat(indent));
            return filters
                .iter()
                .map(|x| pretty2(&to_op(*x), indent + 4, true))
                .collect::<Vec<_>>()
                .join(&i);
        }
    }
    pretty2(&to_op(filter), indent, true)
}

fn pretty2(op: &Op, indent: usize, compose: bool) -> String {
    let ff = |filters: &Vec<_>, n, ind| {
        let ind2 = std::cmp::max(ind, 4);
        let i = format!("\n{}", " ".repeat(ind2));
        let joined = filters
            .iter()
            .map(|x| pretty2(&to_op(*x), ind + 4, true))
            .collect::<Vec<_>>()
            .join(&i);

        format!(
            ":{}[{}{}{}]",
            n,
            &i,
            joined,
            &format!("\n{}", " ".repeat(ind2 - 4))
        )
    };
    match op {
        Op::Compose(filters) => ff(filters, "", indent),
        Op::Subtract(af, bf) => ff(&vec![*af, *bf], "subtract", indent + 4),
        Op::Exclude(bf) => match to_op(*bf) {
            Op::Compose(filters) => ff(&filters, "exclude", indent),
            b => format!(":exclude[{}]", pretty2(&b, indent, false)),
        },
        Op::Chain(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                format!("::{}/", parse::quote_if(&p1.to_string_lossy()))
            }
            (a, Op::Prefix(p)) if compose => {
                format!(
                    "{} = {}",
                    parse::quote_if(&p.to_string_lossy()),
                    pretty2(&a, indent, false)
                )
            }
            (a, b) => format!(
                "{}{}",
                pretty2(&a, indent, false),
                pretty2(&b, indent, false)
            ),
        },
        Op::RegexReplace(replacements) => {
            let v = replacements
                .iter()
                .map(|(regex, r)| {
                    format!(
                        "{}{}:{}",
                        " ".repeat(indent),
                        parse::quote(&regex.to_string()),
                        parse::quote(r)
                    )
                })
                .collect::<Vec<_>>();
            format!(":replace(\n{}\n)", v.join("\n"))
        }
        Op::Squash(Some(ids)) => {
            let mut v = ids
                .iter()
                .map(|(oid, f)| format!("{}{}{}", " ".repeat(indent), &oid.to_string(), spec(*f)))
                .collect::<Vec<_>>();
            v.sort();
            format!(":squash(\n{}\n)", v.join("\n"))
        }
        _ => spec2(op),
    }
}

pub fn nesting(filter: Filter) -> usize {
    nesting2(&to_op(filter))
}

fn nesting2(op: &Op) -> usize {
    match op {
        Op::Compose(filters) => 1 + filters.iter().map(|f| nesting(*f)).fold(0, |a, b| a.max(b)),
        Op::Exclude(filter) => 1 + nesting(*filter),
        Op::Workspace(_) => usize::MAX / 2, // divide by 2 to make sure there is enough headroom to avoid overflows
        Op::Chain(a, b) => 1 + nesting(*a).max(nesting(*b)),
        Op::Subtract(a, b) => 1 + nesting(*a).max(nesting(*b)),
        Op::Rev(filters) => {
            1 + filters
                .values()
                .map(|filter| nesting(*filter))
                .max()
                .unwrap_or(0)
        }
        Op::Join(filters) => {
            1 + filters
                .values()
                .map(|filter| nesting(*filter))
                .max()
                .unwrap_or(0)
        }
        _ => 0,
    }
}

pub fn lazy_refs(filter: Filter) -> Vec<String> {
    lazy_refs2(&to_op(filter))
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
        Op::Exclude(filter) => lazy_refs(*filter),
        Op::Chain(a, b) => {
            let mut av = lazy_refs(*a);
            av.append(&mut lazy_refs(*b));
            av
        }
        Op::Subtract(a, b) => {
            let mut av = lazy_refs(*a);
            av.append(&mut lazy_refs(*b));
            av
        }
        Op::Rev(filters) => lazy_refs2(&Op::Join(filters.clone())),
        Op::Join(filters) => {
            let mut lr = lazy_refs2(&Op::Compose(filters.values().map(|x| *x).collect()));
            lr.extend(filters.keys().filter_map(|x| {
                if let LazyRef::Lazy(s) = x {
                    Some(s.to_owned())
                } else {
                    None
                }
            }));
            lr
        }
        Op::Squash(Some(revs)) => {
            let mut lr = vec![];
            lr.extend(revs.keys().filter_map(|x| {
                if let LazyRef::Lazy(s) = x {
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
        Op::Compose(filters) => Op::Compose(
            filters
                .into_iter()
                .map(|f| resolve_refs(refs, *f))
                .collect(),
        ),
        Op::Exclude(filter) => Op::Exclude(resolve_refs(refs, *filter)),
        Op::Chain(a, b) => Op::Chain(resolve_refs(refs, *a), resolve_refs(refs, *b)),
        Op::Subtract(a, b) => Op::Subtract(resolve_refs(refs, *a), resolve_refs(refs, *b)),
        Op::Rev(filters) => {
            let lr = filters
                .into_iter()
                .map(|(r, f)| {
                    let f = resolve_refs(refs, *f);
                    if let LazyRef::Lazy(s) = r {
                        if let Some(res) = refs.get(s) {
                            (LazyRef::Resolved(*res), f)
                        } else {
                            (r.clone(), f)
                        }
                    } else {
                        (r.clone(), f)
                    }
                })
                .collect();
            Op::Rev(lr)
        }
        Op::Join(filters) => {
            let lr = filters
                .into_iter()
                .map(|(r, f)| {
                    let f = resolve_refs(refs, *f);
                    if let LazyRef::Lazy(s) = r {
                        if let Some(res) = refs.get(s) {
                            (LazyRef::Resolved(*res), f)
                        } else {
                            (r.clone(), f)
                        }
                    } else {
                        (r.clone(), f)
                    }
                })
                .collect();
            Op::Join(lr)
        }
        Op::Squash(Some(filters)) => {
            let lr = filters
                .into_iter()
                .map(|(r, m)| {
                    if let LazyRef::Lazy(s) = r {
                        if let Some(res) = refs.get(s) {
                            (LazyRef::Resolved(*res), m.clone())
                        } else {
                            (r.clone(), m.clone())
                        }
                    } else {
                        (r.clone(), m.clone())
                    }
                })
                .collect();
            Op::Squash(Some(lr))
        }
        _ => op.clone(),
    }
}

/// Compact, single line string representation of a filter so that `parse(spec(F)) == F`
/// Note that this is will not be the best human readable representation. For that see `pretty(...)`
pub fn spec(filter: Filter) -> String {
    let filter = opt::simplify(filter);
    spec2(&to_op(filter))
}

fn spec2(op: &Op) -> String {
    match op {
        Op::Compose(filters) => {
            format!(
                ":[{}]",
                filters
                    .iter()
                    .map(|x| spec(*x))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Op::Subtract(a, b) => {
            format!(":subtract[{},{}]", spec(*a), spec(*b))
        }
        Op::Exclude(b) => {
            format!(":exclude[{}]", spec(*b))
        }
        Op::Rev(filters) => {
            let mut v = filters
                .iter()
                .map(|(k, v)| format!("{}{}", k.to_string(), spec(*v)))
                .collect::<Vec<_>>();
            v.sort();
            format!(":rev({})", v.join(","))
        }
        Op::Join(filters) => {
            let mut v = filters
                .iter()
                .map(|(k, v)| format!("{}{}", k.to_string(), spec(*v)))
                .collect::<Vec<_>>();
            v.sort();
            format!(":join({})", v.join(","))
        }
        Op::Workspace(path) => {
            format!(":workspace={}", parse::quote_if(&path.to_string_lossy()))
        }
        Op::RegexReplace(replacements) => {
            let v = replacements
                .iter()
                .map(|(regex, r)| {
                    format!("{}:{}", parse::quote(&regex.to_string()), parse::quote(r))
                })
                .collect::<Vec<_>>();
            format!(":replace({})", v.join(","))
        }

        Op::Chain(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                format!("::{}/", parse::quote_if(&p1.to_string_lossy()))
            }
            (a, b) => format!("{}{}", spec2(&a), spec2(&b)),
        },

        Op::Nop => ":/".to_string(),
        Op::Empty => ":empty".to_string(),
        Op::Paths => ":PATHS".to_string(),
        Op::Invert => ":INVERT".to_string(),
        Op::Index => ":INDEX".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash(None) => ":SQUASH".to_string(),
        Op::Squash(Some(ids)) => {
            let mut v = ids
                .iter()
                .map(|(oid, f)| format!("{}{}", oid.to_string(), spec(*f)))
                .collect::<Vec<_>>();
            v.sort();
            format!(":squash({})", v.join(","))
        }
        Op::Linear => ":linear".to_string(),
        Op::Unsign => ":unsign".to_string(),
        Op::Subdir(path) => format!(":/{}", parse::quote_if(&path.to_string_lossy())),
        Op::File(path) => format!("::{}", parse::quote_if(&path.to_string_lossy())),
        Op::Prefix(path) => format!(":prefix={}", parse::quote_if(&path.to_string_lossy())),
        Op::Glob(pattern) => format!("::{}", parse::quote_if(pattern)),
        Op::Author(author, email) => {
            format!(":author={};{}", parse::quote(author), parse::quote(email))
        }
        Op::Committer(author, email) => {
            format!(
                ":committer={};{}",
                parse::quote(author),
                parse::quote(email)
            )
        }
        Op::Message(m) => {
            format!(":{}", parse::quote(m))
        }
    }
}

pub fn src_path(filter: Filter) -> std::path::PathBuf {
    src_path2(&to_op(filter))
}

fn src_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Subdir(path) => path.to_owned(),
        Op::File(path) => path.to_owned(),
        Op::Chain(a, b) => src_path(*a).join(src_path(*b)),
        _ => std::path::PathBuf::new(),
    })
}

pub fn dst_path(filter: Filter) -> std::path::PathBuf {
    dst_path2(&to_op(filter))
}

fn dst_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Prefix(path) => path.to_owned(),
        Op::File(path) => path.to_owned(),
        Op::Chain(a, b) => dst_path(*b).join(dst_path(*a)),
        _ => std::path::PathBuf::new(),
    })
}

/// Calculate the filtered commit for `commit`. This can take some time if done
/// for the first time and thus should generally be done asynchronously.
pub fn apply_to_commit(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<git2::Oid> {
    let filter = opt::optimize(filter);
    loop {
        let filtered = apply_to_commit2(&to_op(filter), commit, transaction)?;

        if let Some(id) = filtered {
            return Ok(id);
        }

        let missing = transaction.get_missing();

        // Since 'missing' is sorted by nesting, the first is always the minimal
        let minimal_nesting = missing.get(0).map(|(f, _)| nesting(*f)).unwrap_or(0);

        for (f, i) in missing {
            if nesting(f) != minimal_nesting {
                break;
            }
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
    let f = parse::parse(&tree::get_blob(repo, tree, &path.join("workspace.josh")))
        .unwrap_or_else(|_| to_filter(Op::Empty));

    if let Op::Workspace(p) = to_op(f) {
        Some((
            chain(
                to_filter(Op::Exclude(to_filter(Op::File(path.to_owned())))),
                f,
            ),
            p,
        ))
    } else {
        None
    }
}

fn get_workspace<'a>(repo: &'a git2::Repository, tree: &'a git2::Tree<'a>, path: &Path) -> Filter {
    let ws_path = normalize_path(&path.join("workspace.josh"));
    let ws_id = ok_or!(tree.get_path(&ws_path), {
        return to_filter(Op::Empty);
    })
    .id();
    let ws_blob = tree::get_blob(repo, tree, &ws_path);

    let mut workspaces = WORKSPACES.lock().unwrap();

    if let Some(f) = workspaces.get(&ws_id) {
        *f
    } else {
        let f = parse::parse(&ws_blob).unwrap_or_else(|_| to_filter(Op::Empty));

        let f = if invert(f).is_ok() {
            f
        } else {
            to_filter(Op::Empty)
        };
        workspaces.insert(ws_id, f);
        f
    }
}

pub fn apply_to_commit3(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<bool> {
    Ok(apply_to_commit2(&to_op(filter), commit, transaction)?.is_some())
}

fn apply_to_commit2(
    op: &Op,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<Option<git2::Oid>> {
    let repo = transaction.repo();
    let filter = to_filter(op.clone());

    match &op {
        Op::Nop => return Ok(Some(commit.id())),
        Op::Empty => return Ok(Some(git2::Oid::zero())),

        Op::Chain(a, b) => {
            let r = some_or!(apply_to_commit2(&to_op(*a), commit, transaction)?, {
                return Ok(None);
            });
            return if let Ok(r) = repo.find_commit(r) {
                apply_to_commit2(&to_op(*b), &r, transaction)
            } else {
                Ok(Some(git2::Oid::zero()))
            };
        }
        Op::Squash(None) => {
            return Some(history::rewrite_commit(
                repo,
                commit,
                &[],
                RewriteData {
                    tree: commit.tree()?,
                    author: None,
                    committer: None,
                    message: None,
                },
                true,
            ))
            .transpose();
        }
        Op::Join(refs) => {
            // First loop to populate missing list
            for (&_, f) in refs.iter() {
                transaction.get(*f, commit.id());
            }
            let mut result = commit.id();
            for (&ref combine_tip, f) in refs.iter() {
                if let LazyRef::Resolved(combine_tip) = combine_tip {
                    let old = some_or!(transaction.get(*f, commit.id()), {
                        return Ok(None);
                    });
                    result = history::unapply_filter(
                        transaction,
                        *f,
                        result,
                        old,
                        *combine_tip,
                        false,
                        None,
                        &mut None,
                    )?;
                } else {
                    return Err(josh_error("unresolved lazy ref"));
                }
            }
            transaction.insert(filter, commit.id(), result, true);
            return Ok(Some(result));
        }
        _ => {
            if let Some(oid) = transaction.get(filter, commit.id()) {
                return Ok(Some(oid));
            }
        }
    };

    rs_tracing::trace_scoped!("apply_to_commit", "spec": spec(filter), "commit": commit.id().to_string());

    let rewrite_data = match &to_op(filter) {
        Op::Rev(filters) => {
            let nf = *filters
                .get(&LazyRef::Resolved(git2::Oid::zero()))
                .unwrap_or(&to_filter(Op::Nop));

            let id = commit.id();

            for (&ref filter_tip, startfilter) in filters.iter() {
                let filter_tip = if let LazyRef::Resolved(filter_tip) = filter_tip {
                    filter_tip
                } else {
                    return Err(josh_error("unresolved lazy ref"));
                };
                if *filter_tip == git2::Oid::zero() {
                    continue;
                }
                if !ok_or!(is_ancestor_of(repo, id, *filter_tip), {
                    return Err(josh_error(&format!(
                        "`:rev(...)` with nonexistent OID: {}",
                        filter_tip
                    )));
                }) {
                    continue;
                }
                // Remove this filter but preserve the others.
                let mut f2 = filters.clone();
                f2.remove(&LazyRef::Resolved(*filter_tip));
                f2.insert(LazyRef::Resolved(git2::Oid::zero()), *startfilter);
                let op = if f2.len() == 1 {
                    to_op(*startfilter)
                } else {
                    Op::Rev(f2)
                };
                if let Some(start) = apply_to_commit2(&op, commit, transaction)? {
                    transaction.insert(filter, id, start, true);
                    return Ok(Some(start));
                } else {
                    return Ok(None);
                }
            }

            RewriteData {
                tree: apply(transaction, nf, commit.tree()?)?,
                message: None,
                author: None,
                committer: None,
            }
        }
        Op::Squash(Some(ids)) => {
            if let Some(sq) = ids.get(&LazyRef::Resolved(commit.id())) {
                let oid = if let Some(oid) =
                    apply_to_commit2(&Op::Chain(filter::squash(None), *sq), commit, transaction)?
                {
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
                RewriteData {
                    tree: rc.tree()?,
                    message: rc.message_raw().map(|x| x.to_owned()),
                    author: author,
                    committer: committer,
                }
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
        Op::Linear => {
            let p: Vec<_> = commit.parent_ids().collect();
            if p.is_empty() {
                transaction.insert(filter, commit.id(), commit.id(), true);
                return Ok(Some(commit.id()));
            }
            let parent = some_or!(transaction.get(filter, p[0]), {
                return Ok(None);
            });

            return Some(history::create_filtered_commit(
                commit,
                vec![parent],
                RewriteData {
                    tree: commit.tree()?,
                    author: None,
                    committer: None,
                    message: None,
                },
                transaction,
                filter,
            ))
            .transpose();
        }
        Op::Unsign => {
            let parents: Vec<_> = commit.parent_ids().collect();

            let filtered_parents: Vec<_> = parents
                .iter()
                .map(|p| transaction.get(filter, *p))
                .collect();
            if filtered_parents.iter().any(|p| p.is_none()) {
                return Ok(None);
            }
            let filtered_parents = filtered_parents.iter().map(|p| p.unwrap()).collect();

            return Some(history::remove_commit_signature(
                commit,
                filtered_parents,
                commit.tree()?,
                transaction,
                filter,
            ))
            .transpose();
        }
        Op::Workspace(ws_path) => {
            let normal_parents = commit
                .parent_ids()
                .map(|parent| transaction.get(filter, parent))
                .collect::<Option<Vec<git2::Oid>>>();

            if let Some((redirect, _)) = resolve_workspace_redirect(repo, &commit.tree()?, ws_path)
            {
                if let Some(r) = apply_to_commit2(&to_op(redirect), &commit, transaction)? {
                    transaction.insert(filter, commit.id(), r, true);
                    return Ok(Some(r));
                } else {
                    return Ok(None);
                }
            }

            let normal_parents = some_or!(normal_parents, { return Ok(None) });

            let cw = get_workspace(repo, &commit.tree()?, ws_path);

            let extra_parents = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());

                    let p = if let Some((_, p)) =
                        resolve_workspace_redirect(repo, &parent.tree()?, ws_path)
                    {
                        p
                    } else {
                        ws_path.clone()
                    };

                    let pcw = get_workspace(
                        repo,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        &p,
                    );
                    let f = opt::optimize(to_filter(Op::Subtract(cw, pcw)));

                    apply_to_commit2(&to_op(f), &parent, transaction)
                })
                .collect::<JoshResult<Option<Vec<_>>>>()?;

            let extra_parents = some_or!(extra_parents, { return Ok(None) });

            let filtered_parent_ids = normal_parents
                .into_iter()
                .chain(extra_parents.into_iter())
                .collect();

            let filtered_tree = apply(transaction, filter, commit.tree()?)?;

            return Some(history::create_filtered_commit(
                commit,
                filtered_parent_ids,
                RewriteData {
                    tree: filtered_tree,
                    author: None,
                    committer: None,
                    message: None,
                },
                transaction,
                filter,
            ))
            .transpose();
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
                .collect::<JoshResult<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree = tree::overlay(transaction, filtered_tree, t)?;
            }

            let filtered_tree = repo.find_tree(filtered_tree)?;
            RewriteData {
                tree: filtered_tree,
                author: None,
                committer: None,
                message: None,
            }
        }
        Op::Author(author, email) => RewriteData {
            tree: commit.tree()?,
            author: Some((author.clone(), email.clone())),
            committer: None,
            message: None,
        },
        Op::Committer(author, email) => RewriteData {
            tree: commit.tree()?,
            author: None,
            committer: Some((author.clone(), email.clone())),
            message: None,
        },
        Op::Message(m) => RewriteData {
            tree: commit.tree()?,
            author: None,
            committer: None,
            // Pass the message through `strfmt` to enable future extensions
            message: Some(strfmt::strfmt(
                m,
                &std::collections::HashMap::<String, &dyn strfmt::DisplayStr>::new(),
            )?),
        },
        _ => RewriteData {
            tree: apply(transaction, filter, commit.tree()?)?,
            message: None,
            author: None,
            committer: None,
        },
    };

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
        rewrite_data,
        transaction,
        filter,
    ))
    .transpose()
}

/// Filter a single tree. This does not involve walking history and is thus fast in most cases.
pub fn apply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    apply2(transaction, &to_op(filter), tree)
}

fn apply2<'a>(
    transaction: &'a cache::Transaction,
    op: &Op,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    match op {
        Op::Nop => Ok(tree),
        Op::Empty => return Ok(tree::empty(repo)),
        Op::Fold => Ok(tree),
        Op::Squash(None) => Ok(tree),
        Op::Message(_) => Ok(tree),
        Op::Author(_, _) => Ok(tree),
        Op::Committer(_, _) => Ok(tree),
        Op::Squash(Some(_)) => Err(josh_error("not applicable to tree")),
        Op::Linear => Ok(tree),
        Op::Unsign => Ok(tree),
        Op::Rev(_) => Err(josh_error("not applicable to tree")),
        Op::Join(_) => Err(josh_error("not applicable to tree")),
        Op::RegexReplace(replacements) => {
            let mut t = tree;
            for (regex, replacement) in replacements {
                t = tree::regex_replace(t.id(), regex, replacement, transaction)?;
            }
            Ok(t)
        }

        Op::Glob(pattern) => {
            let pattern = glob::Pattern::new(pattern)?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            tree::remove_pred(
                transaction,
                "",
                tree.id(),
                &|path, isblob| isblob && (pattern.matches_path_with(path, options)),
                to_filter(op.clone()).id(),
            )
        }
        Op::File(path) => {
            let (file, mode) = tree
                .get_path(path)
                .map(|x| (x.id(), x.filemode()))
                .unwrap_or((git2::Oid::zero(), 0o0100644));
            tree::insert(repo, &tree::empty(repo), path, file, mode)
        }

        Op::Subdir(path) => {
            return Ok(tree
                .get_path(path)
                .and_then(|x| repo.find_tree(x.id()))
                .unwrap_or_else(|_| tree::empty(repo)));
        }
        Op::Prefix(path) => tree::insert(repo, &tree::empty(repo), path, tree.id(), 0o0040000),

        Op::Subtract(a, b) => {
            let af = apply(transaction, *a, tree.clone())?;
            let bf = apply(transaction, *b, tree.clone())?;
            let bu = apply(transaction, invert(*b)?, bf)?;
            let ba = apply(transaction, *a, bu)?.id();
            Ok(repo.find_tree(tree::subtract(transaction, af.id(), ba)?)?)
        }
        Op::Exclude(b) => {
            let bf = apply(transaction, *b, tree.clone())?.id();
            Ok(repo.find_tree(tree::subtract(transaction, tree.id(), bf)?)?)
        }

        Op::Paths => tree::pathstree("", tree.id(), transaction),
        Op::Index => tree::trigram_index(transaction, tree),

        Op::Invert => tree::invert_paths(transaction, "", tree),

        Op::Workspace(path) => {
            let wsj_file = to_filter(Op::File(Path::new("workspace.josh").to_owned()));
            let base = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = chain(base, wsj_file);

            if let Some((redirect, _)) = resolve_workspace_redirect(repo, &tree, path) {
                return apply(transaction, redirect, tree);
            }

            apply(
                transaction,
                compose(wsj_file, compose(get_workspace(repo, &tree, path), base)),
                tree,
            )
        }

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| apply(transaction, *f, tree.clone()))
                .collect::<JoshResult<_>>()?;
            let filtered: Vec<_> = filters.iter().zip(filtered.into_iter()).collect();
            tree::compose(transaction, filtered)
        }

        Op::Chain(a, b) => {
            return apply(transaction, *b, apply(transaction, *a, tree)?);
        }
    }
}

/// Calculate a tree with minimal differences from `parent_tree`
/// such that `apply(unapply(tree, parent_tree)) == tree`
pub fn unapply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    if let Ok(inverted) = invert(filter) {
        let filtered = apply(transaction, invert(inverted)?, parent_tree.clone())?;
        let matching = apply(transaction, inverted, filtered)?;
        let stripped = tree::subtract(transaction, parent_tree.id(), matching.id())?;
        let new_tree = apply(transaction, inverted, tree)?;

        return Ok(transaction.repo().find_tree(tree::overlay(
            transaction,
            new_tree.id(),
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

    if let Op::Chain(a, b) = to_op(filter) {
        // If filter "a" is invertable, use "invert(invert(a))" version of it, otherwise use as is
        let a_normalized = if let Ok(a_inverted) = invert(a) {
            invert(a_inverted)?
        } else {
            a
        };
        let filtered_parent_tree = apply(transaction, a_normalized, parent_tree.clone())?;

        return unapply(
            transaction,
            a,
            unapply(transaction, b, tree, filtered_parent_tree)?,
            parent_tree,
        );
    }

    Err(josh_error("filter cannot be unapplied"))
}

fn unapply_workspace<'a>(
    transaction: &'a cache::Transaction,
    op: &Op,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<Option<git2::Tree<'a>>> {
    return match op {
        Op::Workspace(path) => {
            let tree = pre_process_tree(transaction.repo(), tree)?;
            let workspace = get_workspace(transaction.repo(), &tree, Path::new(""));
            let original_workspace = get_workspace(transaction.repo(), &parent_tree, path);

            let root = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = to_filter(Op::File(Path::new("workspace.josh").to_owned()));
            let wsj_file = chain(root, wsj_file);
            let filter = compose(wsj_file, compose(workspace, root));
            let original_filter = compose(wsj_file, compose(original_workspace, root));
            let filtered = apply(transaction, original_filter, parent_tree.clone())?;
            let matching = apply(transaction, invert(original_filter)?, filtered)?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.id())?;
            let new_tree = apply(transaction, invert(filter)?, tree)?;

            let result = transaction.repo().find_tree(tree::overlay(
                transaction,
                new_tree.id(),
                stripped,
            )?)?;

            return Ok(Some(result));
        }
        _ => Ok(None),
    };
}

fn pre_process_tree<'a>(
    repo: &'a git2::Repository,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    let path = Path::new("workspace.josh");
    let ws_file = tree::get_blob(repo, &tree, path);
    let parsed = filter::parse(&ws_file)?;

    if invert(parsed).is_err() {
        return Err(josh_error("Invalid workspace: not reversible"));
    }

    let mut blob = String::new();
    if let Ok(c) = get_comments(&ws_file) {
        if !c.is_empty() {
            blob = c;
        }
    }
    let blob = &format!("{}{}\n", &blob, pretty(parsed, 0));

    let tree = tree::insert(
        repo,
        &tree,
        path,
        repo.blob(blob.as_bytes())?,
        0o0100644, // Should this handle filemode?
    )?;

    Ok(tree)
}

/// Create a filter that is the result of feeding the output of `first` into `second`
pub fn chain(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Chain(first, second)))
}

/// Create a filter that is the result of overlaying the output of `first` onto `second`
pub fn compose(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Compose(vec![first, second])))
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

    let tree = apply(transaction, filter, tree);
    if let Ok(tree) = tree {
        if tree.is_empty() {
            warnings.push(format!("No match for \"{}\"", pretty(filter, 2)));
        }
    }
    warnings
}

pub fn make_permissions_filter(filter: Filter, whitelist: Filter, blacklist: Filter) -> Filter {
    rs_tracing::trace_scoped!("make_permissions_filter");

    let filter = chain(to_filter(Op::Paths), filter);
    let filter = chain(filter, to_filter(Op::Invert));
    let filter = chain(
        filter,
        compose(blacklist, to_filter(Op::Subtract(nop(), whitelist))),
    );
    opt::optimize(filter)
}

/// Check if `commit` is an ancestor of `tip`.
///
/// Creates a cache for a given `tip` so repeated queries with the same `tip` are more efficient.
fn is_ancestor_of(
    repo: &git2::Repository,
    commit: git2::Oid,
    tip: git2::Oid,
) -> Result<bool, git2::Error> {
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
                for parent in repo.find_commit(commit)?.parent_ids() {
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
}

pub fn is_linear(filter: Filter) -> bool {
    match to_op(filter) {
        Op::Linear => true,
        Op::Chain(a, b) => is_linear(a) || is_linear(b),
        _ => false,
    }
}
