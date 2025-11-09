use super::*;
use pest::Parser;
use std::path::Path;
use std::sync::LazyLock;
mod opt;
mod parse;
pub mod persist;
pub mod text;
pub mod tree;

pub use persist::as_tree;
pub use persist::from_tree;

pub use opt::invert;
pub use parse::get_comments;
pub use parse::parse;

static FILTERS: LazyLock<std::sync::Mutex<std::collections::HashMap<Filter, Op>>> =
    LazyLock::new(|| Default::default());
static WORKSPACES: LazyLock<std::sync::Mutex<std::collections::HashMap<git2::Oid, Filter>>> =
    LazyLock::new(|| Default::default());
static ANCESTORS: LazyLock<
    std::sync::Mutex<std::collections::HashMap<git2::Oid, std::collections::HashSet<git2::Oid>>>,
> = LazyLock::new(|| Default::default());

static LEGALIZED: LazyLock<
    std::sync::Mutex<std::collections::HashMap<(Filter, git2::Oid), Filter>>,
> = LazyLock::new(|| Default::default());

/// Match-all regex pattern used as the default for Op::Message when no regex is specified.
/// The pattern `(?s)^.*$` matches any string (including newlines) from start to end.
static MESSAGE_MATCH_ALL_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("(?s)^.*$").unwrap());

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

pub fn sequence_number() -> Filter {
    Filter(git2::Oid::zero())
}

impl std::fmt::Debug for Filter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        to_op(*self).fmt(f)
    }
}

#[derive(Debug)]
pub struct Apply<'a> {
    tree: git2::Tree<'a>,
    commit: git2::Oid,
    pub author: Option<(String, String)>,
    pub committer: Option<(String, String)>,
    pub message: Option<String>,
}

impl<'a> Clone for Apply<'a> {
    fn clone(&self) -> Self {
        Apply {
            tree: self.tree.clone(),
            commit: self.commit.clone(),
            author: self.author.clone(),
            committer: self.committer.clone(),
            message: self.message.clone(),
        }
    }
}

impl<'a> Apply<'a> {
    pub fn from_tree(tree: git2::Tree<'a>) -> Self {
        Apply {
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
        Apply {
            tree,
            author,
            commit: git2::Oid::zero(),
            committer,
            message,
        }
    }

    pub fn from_commit(commit: &git2::Commit<'a>) -> JoshResult<Self> {
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

        Ok(Apply {
            tree,
            commit: commit.id(),
            author,
            committer,
            message,
        })
    }

    pub fn with_author(self, author: (String, String)) -> Self {
        Apply {
            tree: self.tree,
            author: Some(author),
            commit: self.commit,
            committer: self.committer,
            message: self.message,
        }
    }

    pub fn with_committer(self, committer: (String, String)) -> Self {
        Apply {
            tree: self.tree,
            author: self.author,
            commit: self.commit,
            committer: Some(committer),
            message: self.message,
        }
    }

    pub fn with_message(self, message: String) -> Self {
        Apply {
            tree: self.tree,
            author: self.author,
            commit: self.commit,
            committer: self.committer,
            message: Some(message),
        }
    }

    pub fn with_commit(self, commit: git2::Oid) -> Self {
        Apply {
            tree: self.tree,
            author: self.author,
            commit: commit,
            committer: self.committer,
            message: self.message,
        }
    }

    pub fn with_tree(self, tree: git2::Tree<'a>) -> Self {
        Apply {
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

pub fn nop() -> Filter {
    to_filter(Op::Nop)
}

pub fn empty() -> Filter {
    to_filter(Op::Empty)
}

pub fn message(m: &str) -> Filter {
    to_filter(Op::Message(m.to_string(), MESSAGE_MATCH_ALL_REGEX.clone()))
}

pub fn file(path: impl Into<std::path::PathBuf>) -> Filter {
    let p = path.into();
    to_filter(Op::File(p.clone(), p))
}

pub fn hook(h: &str) -> Filter {
    to_filter(Op::Hook(h.to_string()))
}

pub fn squash(ids: Option<&[(git2::Oid, Filter)]>) -> Filter {
    if let Some(ids) = ids {
        to_filter(Op::Squash(Some(
            ids.iter()
                .map(|(x, y)| (LazyRef::Resolved(*x), *y))
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
            Ok(LazyRef::Resolved(oid))
        } else {
            Err(josh_error(&format!("invalid ref: {:?}", s)))
        }
    }
}

#[derive(Clone, Debug)]
enum Op {
    Nop,
    Empty,
    Fold,
    Paths,
    Adapt(String),
    Link(String),
    Unlink,
    Export,
    Embed(std::path::PathBuf),

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
    Prune,
    Unsign,

    RegexReplace(Vec<(regex::Regex, String)>),

    Hook(String),

    Index,
    Invert,

    File(std::path::PathBuf, std::path::PathBuf), // File(dest_path, source_path)
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),
    Stored(std::path::PathBuf),

    Pattern(String),
    Message(String, regex::Regex),

    HistoryConcat(LazyRef, Filter),
    Unapply(LazyRef, Filter),

    Compose(Vec<Filter>),
    Chain(Filter, Filter),
    Subtract(Filter, Filter),
    Exclude(Filter),
    Pin(Filter),
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
        Op::Pin(filter) => match to_op(*filter) {
            Op::Compose(filters) => ff(&filters, "pin", indent),
            b => format!(":pin[{}]", pretty2(&b, indent, false)),
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
        Op::Exclude(filter) | Op::Pin(filter) => lazy_refs(*filter),
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
        Op::Rev(filters) => {
            let mut lr = lazy_refs2(&Op::Compose(filters.values().copied().collect()));
            lr.extend(filters.keys().filter_map(|x| {
                if let LazyRef::Lazy(s) = x {
                    Some(s.to_owned())
                } else {
                    None
                }
            }));
            lr.sort();
            lr.dedup();
            lr
        }
        Op::HistoryConcat(r, f) => {
            let mut lr = Vec::new();
            if let LazyRef::Lazy(s) = r {
                lr.push(s.to_owned());
            }
            lr.append(&mut lazy_refs(*f));
            lr
        }
        Op::Unapply(r, f) => {
            let mut lr = Vec::new();
            if let LazyRef::Lazy(s) = r {
                lr.push(s.to_owned());
            }
            lr.append(&mut lazy_refs(*f));
            lr
        }
        Op::Join(filters) => {
            let mut lr = lazy_refs2(&Op::Compose(filters.values().copied().collect()));
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
        Op::Compose(filters) => {
            Op::Compose(filters.iter().map(|f| resolve_refs(refs, *f)).collect())
        }
        Op::Exclude(filter) => Op::Exclude(resolve_refs(refs, *filter)),
        Op::Pin(filter) => Op::Pin(resolve_refs(refs, *filter)),
        Op::Chain(a, b) => Op::Chain(resolve_refs(refs, *a), resolve_refs(refs, *b)),
        Op::Subtract(a, b) => Op::Subtract(resolve_refs(refs, *a), resolve_refs(refs, *b)),
        Op::Rev(filters) => {
            let lr = filters
                .iter()
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
        Op::HistoryConcat(r, filter) => {
            let f = resolve_refs(refs, *filter);
            let resolved_ref = if let LazyRef::Lazy(s) = r {
                if let Some(res) = refs.get(s) {
                    LazyRef::Resolved(*res)
                } else {
                    r.clone()
                }
            } else {
                r.clone()
            };
            Op::HistoryConcat(resolved_ref, f)
        }
        Op::Unapply(r, f) => {
            let f = resolve_refs(refs, *f);
            let resolved_ref = if let LazyRef::Lazy(s) = r {
                if let Some(res) = refs.get(s) {
                    LazyRef::Resolved(*res)
                } else {
                    r.clone()
                }
            } else {
                r.clone()
            };
            Op::Unapply(resolved_ref, f)
        }
        Op::Join(filters) => {
            let lr = filters
                .iter()
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

/// Compact, single line string representation of a filter so that `parse(spec(F)) == F`
/// Note that this is will not be the best human readable representation. For that see `pretty(...)`
pub fn spec(filter: Filter) -> String {
    if filter == sequence_number() {
        return "sequence_number".to_string();
    }
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
        Op::Pin(filter) => {
            format!(":pin[{}]", spec(*filter))
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
        Op::Stored(path) => {
            format!(":+{}", parse::quote_if(&path.to_string_lossy()))
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
        Op::Adapt(adapter) => format!(":adapt={}", adapter),
        Op::Link(mode) => format!(":link={}", mode),
        Op::Export => ":export".to_string(),
        Op::Unlink => ":unlink".to_string(),
        Op::Subdir(path) => format!(":/{}", parse::quote_if(&path.to_string_lossy())),
        Op::File(dest_path, source_path) => {
            if source_path == dest_path {
                format!("::{}", parse::quote_if(&dest_path.to_string_lossy()))
            } else {
                format!(
                    "::{}={}",
                    parse::quote_if(&dest_path.to_string_lossy()),
                    parse::quote_if(&source_path.to_string_lossy())
                )
            }
        }
        Op::Prune => ":prune=trivial-merge".to_string(),
        Op::Prefix(path) => format!(":prefix={}", parse::quote_if(&path.to_string_lossy())),
        Op::Pattern(pattern) => format!("::{}", parse::quote_if(pattern)),
        Op::Embed(path) => {
            format!(":embed={}", parse::quote_if(&path.to_string_lossy()),)
        }
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
        Op::Message(m, r) if r.as_str() == MESSAGE_MATCH_ALL_REGEX.as_str() => {
            format!(":{}", parse::quote(m))
        }
        Op::Message(m, r) => {
            format!(":{};{}", parse::quote(m), parse::quote(r.as_str()))
        }
        Op::HistoryConcat(r, filter) => {
            format!(":concat({}{})", r.to_string(), spec(*filter))
        }
        Op::Unapply(r, filter) => {
            format!(":unapply({}{})", r.to_string(), spec(*filter))
        }
        Op::Hook(hook) => {
            format!(":hook={}", parse::quote(hook))
        }
    }
}

pub fn src_path(filter: Filter) -> std::path::PathBuf {
    src_path2(&to_op(filter))
}

fn src_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Subdir(path) => path.to_owned(),
        Op::File(_, source_path) => source_path.to_owned(),
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
        Op::File(dest_path, _) => dest_path.to_owned(),
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

        for (f, i) in missing {
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
        Some((chain(to_filter(Op::Exclude(file(path))), f), p))
    } else {
        None
    }
}

fn get_workspace<'a>(
    transaction: &cache::Transaction,
    tree: &'a git2::Tree<'a>,
    path: &Path,
) -> Filter {
    let wsj_file = file("workspace.josh");
    let base = to_filter(Op::Subdir(path.to_owned()));
    let wsj_file = chain(base, wsj_file);
    compose(
        wsj_file,
        compose(
            get_filter(transaction, tree, &path.join("workspace.josh")),
            base,
        ),
    )
}

fn get_stored<'a>(
    transaction: &cache::Transaction,
    tree: &'a git2::Tree<'a>,
    path: &Path,
) -> Filter {
    let stored_path = path.with_extension("josh");
    let sj_file = file(stored_path.clone());
    compose(sj_file, get_filter(transaction, tree, &stored_path))
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
        let f = parse::parse(&ws_blob).unwrap_or_else(|_| to_filter(Op::Empty));
        let f = legalize_stored(transaction, f, tree);

        let f = if invert(f).is_ok() {
            f
        } else {
            to_filter(Op::Empty)
        };
        WORKSPACES.lock().unwrap().insert(ws_id, f);
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
                Apply::from_commit(commit)?,
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
            for (combine_tip, f) in refs.iter() {
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
                        history::OrphansMode::Keep,
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

            for (filter_tip, startfilter) in filters.iter() {
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

            apply(transaction, nf, Apply::from_commit(commit)?)?
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
                Apply::from_tree_with_metadata(
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
                Apply::from_commit(commit)?,
                transaction,
                filter,
            ))
            .transpose();
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

            Apply::from_commit(commit)?
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
        Op::Export => {
            let filtered_parent_ids = {
                commit
                    .parents()
                    .map(|x| transaction.get(filter, x.id()))
                    .collect::<Option<_>>()
            };

            let mut filtered_parent_ids: Vec<git2::Oid> =
                some_or!(filtered_parent_ids, { return Ok(None) });

            // TODO: remove all parents that don't have a .josh-link.toml

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
            //         return Err(josh_error("missing commit"));
            //     }

            if let Some(link_file) = read_josh_link(
                repo,
                &commit.tree()?,
                &std::path::PathBuf::new(),
                ".josh-link.toml",
            ) {
                if filtered_parent_ids.contains(&link_file.commit.0) {
                    while filtered_parent_ids[0] != link_file.commit.0 {
                        filtered_parent_ids.rotate_right(1);
                    }
                }
            }

            return Some(history::create_filtered_commit(
                commit,
                filtered_parent_ids,
                apply(transaction, filter, Apply::from_commit(commit)?)?,
                transaction,
                filter,
            ))
            .transpose();
        }
        Op::Unlink => {
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
                if let Some(cmt) =
                    transaction.get(to_filter(Op::Prefix(link_path)), link_file.commit.0)
                {
                    link_parents.push(cmt);
                } else {
                    return Ok(None);
                }
            }

            let new_tree = apply(transaction, filter, Apply::from_commit(commit)?)?;

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
        Op::Link(mode) if mode == "embedded" => {
            let normal_parents = commit
                .parent_ids()
                .map(|parent| transaction.get(filter, parent))
                .collect::<Option<Vec<git2::Oid>>>();

            let normal_parents = some_or!(normal_parents, { return Ok(None) });

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

            let v = links_from_roots(repo, &commit.tree()?, roots)?;

            let extra_parents = {
                let mut extra_parents = vec![];
                for (root, _link_file) in v {
                    let embeding = some_or!(
                        apply_to_commit2(
                            &Op::Chain(message("{commit}"), file(root.join(".josh-link.toml"))),
                            &commit,
                            transaction
                        )?,
                        {
                            return Ok(None);
                        }
                    );

                    let f = to_filter(Op::Embed(root));
                    /* let f = filter::chain(link_file.filter, to_filter(Op::Prefix(root))); */
                    /* let scommit = repo.find_commit(link_file.commit.0)?; */

                    let embeding = repo.find_commit(embeding)?;
                    let r = some_or!(apply_to_commit2(&to_op(f), &embeding, transaction)?, {
                        return Ok(None);
                    });

                    extra_parents.push(r);
                }

                extra_parents
            };

            let filtered_tree = apply(transaction, filter, Apply::from_commit(commit)?)?;
            let filtered_parent_ids = normal_parents
                .into_iter()
                .chain(extra_parents)
                .collect::<Vec<_>>();

            return Some(history::create_filtered_commit(
                commit,
                filtered_parent_ids.clone(),
                filtered_tree,
                transaction,
                filter,
            ))
            .transpose();
        }
        Op::Workspace(ws_path) => {
            if let Some((redirect, _)) = resolve_workspace_redirect(repo, &commit.tree()?, ws_path)
            {
                if let Some(r) = apply_to_commit2(&to_op(redirect), commit, transaction)? {
                    transaction.insert(filter, commit.id(), r, true);
                    return Ok(Some(r));
                } else {
                    return Ok(None);
                }
            }

            let commit_filter = get_workspace(transaction, &commit.tree()?, &ws_path);

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcw = get_workspace(
                        transaction,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        &ws_path,
                    );
                    Ok((parent, pcw))
                })
                .collect::<JoshResult<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        Op::Stored(s_path) => {
            let commit_filter = get_stored(transaction, &commit.tree()?, &s_path);

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcs = get_stored(
                        transaction,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        &s_path,
                    );
                    Ok((parent, pcs))
                })
                .collect::<JoshResult<Vec<_>>>()?;

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
                .collect::<JoshResult<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree = tree::overlay(transaction, filtered_tree, t)?;
            }

            let filtered_tree = repo.find_tree(filtered_tree)?;
            Apply::from_commit(commit)?.with_tree(filtered_tree)
        }
        Op::Hook(hook) => {
            let commit_filter = transaction.lookup_filter_hook(&hook, commit.id())?;

            let parent_filters = commit
                .parents()
                .map(|parent| {
                    let pcw = transaction.lookup_filter_hook(&hook, parent.id())?;
                    Ok((parent, pcw))
                })
                .collect::<JoshResult<Vec<_>>>()?;

            return per_rev_filter(transaction, commit, filter, commit_filter, parent_filters);
        }
        Op::Unapply(target, uf) => {
            if let LazyRef::Resolved(target) = target {
                /* dbg!(target); */
                let target = repo.find_commit(*target)?;
                if let Some(parent) = target.parents().next() {
                    let ptree = apply(transaction, *uf, Apply::from_commit(&parent)?)?;
                    if let Some(link) = read_josh_link(
                        repo,
                        &ptree.tree(),
                        &std::path::PathBuf::new(),
                        ".josh-link.toml",
                    ) {
                        if commit.id() == link.commit.0 {
                            let unapply =
                                to_filter(Op::Unapply(LazyRef::Resolved(parent.id()), *uf));
                            let r = some_or!(transaction.get(unapply, link.commit.0), {
                                return Ok(None);
                            });
                            transaction.insert(filter, commit.id(), r, true);
                            return Ok(Some(r));
                        }
                    }
                }
            } else {
                return Err(josh_error("unresolved lazy ref"));
            }
            /* dbg!("FALLTHROUGH"); */
            apply(
                transaction,
                filter,
                Apply::from_commit(commit)?, /* Apply::from_commit(commit)?.with_parents(filtered_parent_ids), */
            )?
            /* Apply::from_commit(commit)? */
        }
        Op::Embed(path) => {
            let subdir = to_filter(Op::Subdir(path.clone()));
            let unapply = to_filter(Op::Unapply(LazyRef::Resolved(commit.id()), subdir));

            /* dbg!("embed"); */
            /* dbg!(&path); */
            if let Some(link) = read_josh_link(repo, &commit.tree()?, &path, ".josh-link.toml") {
                /* dbg!(&link); */
                let r = some_or!(transaction.get(unapply, link.commit.0), {
                    return Ok(None);
                });
                transaction.insert(filter, commit.id(), r, true);
                return Ok(Some(r));
            } else {
                return Ok(Some(git2::Oid::zero()));
            }
        }

        Op::HistoryConcat(c, f) => {
            if let LazyRef::Resolved(c) = c {
                let a = apply_to_commit2(&to_op(*f), &repo.find_commit(*c)?, transaction)?;
                let a = some_or!(a, { return Ok(None) });
                if commit.id() == a {
                    transaction.insert(filter, commit.id(), *c, true);
                    return Ok(Some(*c));
                }
            } else {
                return Err(josh_error("unresolved lazy ref"));
            }
            Apply::from_commit(commit)?
        }
        _ => apply(transaction, filter, Apply::from_commit(commit)?)?,
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

/// Filter a single tree. This does not involve walking history and is thus fast in most cases.
pub fn apply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    x: Apply<'a>,
) -> JoshResult<Apply<'a>> {
    apply2(transaction, &to_op(filter), x)
}

fn extract_submodule_commits<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree<'a>,
) -> JoshResult<std::collections::BTreeMap<std::path::PathBuf, (git2::Oid, ParsedSubmoduleEntry)>> {
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

fn get_link_roots<'a>(
    _repo: &'a git2::Repository,
    transaction: &'a cache::Transaction,
    tree: &'a git2::Tree<'a>,
) -> JoshResult<Vec<std::path::PathBuf>> {
    let link_filter = to_filter(Op::Pattern("**/.josh-link.toml".to_string()));
    let link_tree = apply(transaction, link_filter, Apply::from_tree(tree.clone()))?;

    let mut roots = vec![];
    link_tree
        .tree()
        .walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            let root = root.trim_matches('/');
            let root = std::path::PathBuf::from(root);
            if entry.name() == Some(".josh-link.toml") {
                roots.push(root);
            }
            0
        })?;

    Ok(roots)
}

fn links_from_roots<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree<'a>,
    roots: Vec<std::path::PathBuf>,
) -> JoshResult<Vec<(std::path::PathBuf, JoshLinkFile)>> {
    let mut v = vec![];
    for root in roots {
        if let Some(link_file) = read_josh_link(repo, tree, &root, ".josh-link.toml") {
            v.push((root, link_file));
        }
    }
    Ok(v)
}

fn read_josh_link<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree<'a>,
    root: &std::path::Path,
    filename: &str,
) -> Option<JoshLinkFile> {
    let link_path = root.join(filename);
    let link_entry = tree.get_path(&link_path).ok()?;
    let link_blob = repo.find_blob(link_entry.id()).ok()?;
    let b = std::str::from_utf8(link_blob.content())
        .map_err(|e| josh_error(&format!("invalid utf8 in {}: {}", filename, e)))
        .ok()?;
    let link_file: JoshLinkFile = toml::from_str(b)
        .map_err(|e| josh_error(&format!("invalid toml in {}: {}", filename, e)))
        .ok()?;
    Some(link_file)
}

fn apply2<'a>(transaction: &'a cache::Transaction, op: &Op, x: Apply<'a>) -> JoshResult<Apply<'a>> {
    let repo = transaction.repo();
    match op {
        Op::Nop => Ok(x),
        Op::Empty => Ok(x.with_tree(tree::empty(repo))),
        Op::Fold => Ok(x),
        Op::Squash(None) => Ok(x),
        Op::Author(author, email) => Ok(x.with_author((author.clone(), email.clone()))),
        Op::Committer(author, email) => Ok(x.with_committer((author.clone(), email.clone()))),
        Op::Squash(Some(_)) => Err(josh_error("not applicable to tree")),
        Op::Message(m, r) => {
            let tree_id = x.tree().id().to_string();
            let commit = x.commit;
            let commit_id = commit.to_string();
            let mut hm = std::collections::HashMap::<String, String>::new();
            hm.insert("tree".to_string(), tree_id);
            hm.insert("commit".to_string(), commit_id);

            let message = if let Some(ref m) = x.message {
                m.to_string()
            } else {
                if let Ok(c) = transaction.repo().find_commit(commit) {
                    c.message_raw().unwrap_or_default().to_string()
                } else {
                    "".to_string()
                }
            };

            Ok(x.with_message(text::transform_with_template(&r, &m, &message, &hm)?))
        }
        Op::HistoryConcat(..) => Ok(x),
        Op::Linear => Ok(x),
        Op::Prune => Ok(x),
        Op::Unsign => Ok(x),
        Op::Adapt(adapter) => {
            let mut result_tree = x.tree().clone();
            match adapter.as_ref() {
                "submodules" => {
                    // Extract submodule commits
                    let submodule_commits = extract_submodule_commits(repo, &result_tree)?;

                    // Process each submodule commit
                    for (submodule_path, (commit_oid, meta)) in submodule_commits {
                        let prefix_filter = to_filter(Op::Nop);
                        let link_file = JoshLinkFile {
                            remote: meta.url.clone(),
                            filter: prefix_filter,
                            branch: "HEAD".to_string(),
                            commit: Oid(commit_oid),
                        };
                        result_tree = tree::insert(
                            repo,
                            &result_tree,
                            &submodule_path.join(".josh-link.toml"),
                            repo.blob(toml::to_string(&link_file)?.as_bytes())?,
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
                _ => return Err(josh_error(&format!("unknown adapter {:?}", adapter))),
            }

            // Remove .gitmodules file by setting it to zero OID
            result_tree = tree::insert(
                repo,
                &result_tree,
                std::path::Path::new(".gitmodules"),
                git2::Oid::zero(),
                0o0100644,
            )?;

            Ok(x.with_tree(result_tree))
        }
        Op::Export => {
            let tree = x.tree().clone();
            Ok(x.with_tree(tree::insert(
                repo,
                &tree,
                &std::path::Path::new(".josh-link.toml"),
                git2::Oid::zero(),
                0o0100644,
            )?))
        }
        Op::Unlink => {
            let mut result_tree = x.tree.clone();
            for (link_path, link_file) in find_link_files(&repo, &result_tree)?.iter() {
                result_tree =
                    tree::insert(repo, &result_tree, &link_path, git2::Oid::zero(), 0o0100644)?;
                result_tree = tree::insert(
                    repo,
                    &result_tree,
                    &link_path.join(".josh-link.toml"),
                    repo.blob(toml::to_string(&link_file)?.as_bytes())?,
                    0o0100644,
                )?;
            }
            Ok(x.with_tree(result_tree))
        }
        Op::Link(_) => {
            let roots = get_link_roots(repo, transaction, &x.tree())?;
            let v = links_from_roots(repo, &x.tree(), roots)?;
            let mut result_tree = x.tree().clone();

            for (root, link_file) in v {
                let submodule_tree = repo.find_commit(link_file.commit.0)?.tree()?;
                let submodule_tree = apply(
                    transaction,
                    link_file.filter,
                    Apply::from_tree(submodule_tree),
                )
                .unwrap();

                result_tree = tree::insert(
                    repo,
                    &result_tree,
                    &root,
                    submodule_tree.tree().id(),
                    0o0040000, // Tree mode
                )?;
                result_tree = tree::insert(
                    repo,
                    &result_tree,
                    &root.join(".josh-link.toml"),
                    repo.blob(toml::to_string(&link_file)?.as_bytes())?,
                    0o0100644,
                )?;
            }

            Ok(x.with_tree(result_tree))
        }
        Op::Rev(_) => Err(josh_error("not applicable to tree")),
        Op::Join(_) => Err(josh_error("not applicable to tree")),
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

        Op::Workspace(path) => apply(transaction, get_workspace(transaction, &x.tree(), &path), x),
        Op::Stored(path) => apply(transaction, get_stored(transaction, &x.tree(), &path), x),

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| apply(transaction, *f, x.clone()))
                .collect::<JoshResult<_>>()?;
            let filtered: Vec<_> = filters
                .iter()
                .zip(filtered.iter().map(|t| t.tree().clone()))
                .collect();
            Ok(x.with_tree(tree::compose(transaction, filtered)?))
        }

        Op::Chain(a, b) => {
            return apply(transaction, *b, apply(transaction, *a, x.clone())?);
        }
        Op::Hook(_) => Err(josh_error("not applicable to tree")),

        Op::Embed(..) => Err(josh_error("not applicable to tree")),
        Op::Unapply(target, uf) => {
            if let LazyRef::Resolved(target) = target {
                let target = repo.find_commit(*target)?;
                let target = git2::Oid::from_str(target.message().unwrap())?;
                let target = repo.find_commit(target)?;
                /* dbg!(&uf); */
                Ok(Apply::from_tree(filter::unapply(
                    transaction,
                    *uf,
                    x.tree().clone(),
                    target.tree()?,
                )?))
            } else {
                return Err(josh_error("unresolved lazy ref"));
            }
        }
        Op::Pin(_) => Ok(x),
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
        let filtered = apply(
            transaction,
            invert(inverted)?,
            Apply::from_tree(parent_tree.clone()),
        )?;
        let matching = apply(transaction, inverted, filtered.clone())?;
        let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
        let x = apply(transaction, inverted, Apply::from_tree(tree))?;

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

    if let Op::Chain(a, b) = to_op(filter) {
        // If filter "a" is invertable, use "invert(invert(a))" version of it, otherwise use as is
        let a_normalized = if let Ok(a_inverted) = invert(a) {
            invert(a_inverted)?
        } else {
            a
        };
        let filtered_parent_tree = apply(
            transaction,
            a_normalized,
            Apply::from_tree(parent_tree.clone()),
        )?
        .into_tree();

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
            let wsj_file = chain(root, wsj_file);
            let filter = compose(wsj_file, compose(workspace, root));
            let original_filter = compose(wsj_file, compose(original_workspace, root));
            let filtered = apply(
                transaction,
                original_filter,
                Apply::from_tree(parent_tree.clone()),
            )?;
            let matching = apply(transaction, invert(original_filter)?, filtered.clone())?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
            let x = apply(transaction, invert(filter)?, Apply::from_tree(tree))?;

            let result = transaction.repo().find_tree(tree::overlay(
                transaction,
                x.tree().id(),
                stripped,
            )?)?;

            Ok(Some(result))
        }
        Op::Stored(path) => {
            let stored_path = path.with_extension("josh");
            let stored = get_filter(transaction, &tree, &stored_path);
            let original_stored = get_filter(transaction, &parent_tree, &stored_path);

            let sj_file = file(stored_path.clone());
            let filter = compose(sj_file, stored);
            let original_filter = compose(sj_file, original_stored);
            let filtered = apply(
                transaction,
                original_filter,
                Apply::from_tree(parent_tree.clone()),
            )?;
            let matching = apply(transaction, invert(original_filter)?, filtered.clone())?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.tree().id())?;
            let x = apply(transaction, invert(filter)?, Apply::from_tree(tree))?;

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
        git2::FileMode::Blob.into(), // Should this handle filemode?
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

    if let Op::Stored(path) = to_op(filter) {
        let stored_path = path.with_extension("josh");
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

    let x = apply(transaction, filter, Apply::from_tree(tree));
    if let Ok(x) = x {
        if x.tree().is_empty() {
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

pub fn is_linear(filter: Filter) -> bool {
    match to_op(filter) {
        Op::Linear => true,
        Op::Chain(a, b) => is_linear(a) || is_linear(b),
        _ => false,
    }
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
        Op::Chain(a, b) => to_filter(Op::Chain(legalize_pin(a, c), legalize_pin(b, c))),
        Op::Subtract(a, b) => to_filter(Op::Subtract(legalize_pin(a, c), legalize_pin(b, c))),
        Op::Exclude(f) => to_filter(Op::Exclude(legalize_pin(f, c))),
        Op::Pin(f) => c(f),
        _ => f,
    }
}

fn legalize_stored(t: &cache::Transaction, f: Filter, tree: &git2::Tree) -> Filter {
    legalize_stored2(t, f, tree, &mut *LEGALIZED.lock().unwrap()).unwrap_or_else(|_| empty())
}

fn legalize_stored2(
    t: &cache::Transaction,
    f: Filter,
    tree: &git2::Tree,
    hm: &mut std::collections::HashMap<(Filter, git2::Oid), Filter>,
) -> JoshResult<Filter> {
    if let Some(f) = hm.get(&(f, tree.id())) {
        return Ok(*f);
    }

    // Put an entry into the hashtable to prevent infinite recursion.
    // If we get called with the same arguments again before we return,
    // Above check breaks the recursion.
    hm.insert((f, tree.id()), empty());

    let r = match to_op(f) {
        Op::Compose(f) => {
            let f = f
                .into_iter()
                .map(|f| legalize_stored2(t, f, tree, hm))
                .collect::<JoshResult<Vec<_>>>()?;
            to_filter(Op::Compose(f))
        }
        Op::Chain(a, b) => {
            let first = legalize_stored2(t, a, tree, hm)?;
            let second = legalize_stored2(
                t,
                b,
                &apply(t, first, Apply::from_tree(tree.clone()))?.tree,
                hm,
            )?;
            to_filter(Op::Chain(first, second))
        }
        Op::Subtract(a, b) => to_filter(Op::Subtract(
            legalize_stored2(t, a, tree, hm)?,
            legalize_stored2(t, b, tree, hm)?,
        )),
        Op::Exclude(f) => to_filter(Op::Exclude(legalize_stored2(t, f, tree, hm)?)),
        Op::Pin(f) => to_filter(Op::Pin(legalize_stored2(t, f, tree, hm)?)),
        Op::Stored(path) => get_stored(t, tree, &path),
        _ => f,
    };

    hm.insert((f, tree.id()), r);

    Ok(r)
}

fn per_rev_filter(
    transaction: &cache::Transaction,
    commit: &git2::Commit,
    filter: Filter,
    commit_filter: Filter,
    parent_filters: Vec<(git2::Commit, Filter)>,
) -> JoshResult<Option<git2::Oid>> {
    // Compute the difference between the current commit's filter and each parent's filter.
    // This determines what new content should be contributed by that parent in the filtered history.
    let extra_parents = parent_filters
        .into_iter()
        .map(|(parent, pcw)| {
            let f = opt::optimize(to_filter(Op::Subtract(commit_filter, pcw)));
            apply_to_commit2(&to_op(f), &parent, transaction)
        })
        .collect::<JoshResult<Option<Vec<_>>>>()?;

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
                Apply::from_commit(commit)?,
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

    let mut tree_data = apply(transaction, commit_filter, Apply::from_commit(commit)?)?;

    if let Some((pin_subtract, pin_overlay)) = pin_details {
        let with_exclude = tree::subtract(transaction, tree_data.tree().id(), pin_subtract)?;
        let with_overlay = tree::overlay(transaction, pin_overlay, with_exclude)?;

        tree_data = tree_data.with_tree(transaction.repo().find_tree(with_overlay)?);
    }

    return Some(history::create_filtered_commit(
        commit,
        filtered_parent_ids,
        tree_data,
        transaction,
        filter,
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
}
