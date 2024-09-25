#![warn(unused_extern_crates)]

#[macro_export]
macro_rules! some_or {
    ($e:expr, $b:block) => {
        if let Some(x) = $e {
            x
        } else {
            $b
        }
    };
}

#[macro_export]
macro_rules! ok_or {
    ($e:expr, $b:block) => {
        if let Ok(x) = $e {
            x
        } else {
            $b
        }
    };
}

#[macro_use]
extern crate rs_tracing;

pub mod cache;
pub mod filter;
pub mod history;
pub mod housekeeping;
pub mod shell;

pub struct Change {
    pub author: String,
    pub id: Option<String>,
    pub commit: git2::Oid,
}

impl Change {
    fn new(commit: git2::Oid) -> Self {
        Self {
            author: Default::default(),
            id: Default::default(),
            commit,
        }
    }
}

#[derive(
    Clone, Hash, PartialEq, Eq, Copy, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct Oid(git2::Oid);

impl Default for Oid {
    fn default() -> Self {
        Oid(git2::Oid::zero())
    }
}

impl std::convert::TryFrom<String> for Oid {
    type Error = JoshError;
    fn try_from(s: String) -> JoshResult<Oid> {
        Ok(Oid(git2::Oid::from_str(&s)?))
    }
}

impl From<Oid> for String {
    fn from(val: Oid) -> Self {
        val.0.to_string()
    }
}

impl From<Oid> for git2::Oid {
    fn from(val: Oid) -> Self {
        val.0
    }
}

impl From<git2::Oid> for Oid {
    fn from(oid: git2::Oid) -> Self {
        Self(oid)
    }
}

/// Determine the josh version number with the following precedence:
///
/// 1. If in a git checkout, and `git` binary is present, use the
///    commit ID or nearest tag.
/// 2. If not in a git checkout, use the value of the `JOSH_VERSION`
///    environment variable (e.g. a build from a tarball).
/// 3. If neither options work, fall back to the string "unknown".
///
/// This is used to display version numbers at runtime in different
/// josh components.
pub const VERSION: &str = git_version::git_version!(
    args = ["--tags", "--always", "--dirty=-modified"],
    fallback = match option_env!("JOSH_VERSION") {
        Some(v) => v,
        None => "unknown",
    },
);

const FRAGMENT: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b'/')
    .add(b'*')
    .add(b' ')
    .add(b'~')
    .add(b'^')
    .add(b':')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'{')
    .add(b'}')
    .add(b'@')
    .add(b'\\');

pub fn to_ns(path: &str) -> String {
    return percent_encoding::utf8_percent_encode(path.trim_matches('/'), FRAGMENT).to_string();
}

pub fn from_ns(path: &str) -> String {
    return percent_encoding::percent_decode_str(path.trim_matches('/'))
        .decode_utf8_lossy()
        .to_string();
}

pub fn to_filtered_ref(upstream_repo: &str, filter_spec: &str) -> String {
    format!(
        "josh/filtered/{}/{}",
        to_ns(upstream_repo),
        to_ns(filter_spec)
    )
}

#[derive(Debug, Clone)]
pub struct JoshError(pub String);

pub fn josh_error(s: &str) -> JoshError {
    JoshError(s.to_owned())
}

impl std::fmt::Display for JoshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JoshError({})", self.0)
    }
}

pub type JoshResult<T> = Result<T, JoshError>;

impl<T> From<T> for JoshError
where
    T: std::error::Error,
{
    fn from(item: T) -> Self {
        let bt = backtrace::Backtrace::new();
        tracing::event!(tracing::Level::ERROR, item = ?item, backtrace = format!("{:?}", bt), error = true);
        log::error!("JoshError: {:?}", item);
        log::error!("Backtrace: {:?}", bt);
        josh_error(&format!("converted {:?}", item))
    }
}

#[macro_use]
extern crate lazy_static;

#[macro_export]
macro_rules! regex_parsed {
    ($name:ident, $re:literal,  [$( $i:ident ),+]) => {

        struct $name {
            $(
                $i: String,
            )+
        }

impl $name {
    fn from_str(path: &str) -> Option<$name> {

lazy_static::lazy_static! {
    static ref REGEX: regex::Regex =
        regex::Regex::new($re)
            .expect("can't compile regex");
}
        let caps = REGEX.captures(&path)?;
        let as_str = |x: regex::Match| x.as_str().to_owned();

        return Some($name {
            $(
            $i: caps.name(stringify!($i)).map(as_str).unwrap_or("".to_owned()),
            )+
        });
    }
}
    }
}

pub fn get_change_id(commit: &git2::Commit, sha: git2::Oid) -> Change {
    let mut change = Change::new(sha);
    change.author = commit.author().email().unwrap_or("").to_string();

    for line in commit.message().unwrap_or("").split('\n') {
        if line.starts_with("Change: ") {
            change.id = Some(line.replacen("Change: ", "", 1));
            // If there is a "Change-Id" as well, it will take precedence
        }
        if line.starts_with("Change-Id: ") {
            change.id = Some(line.replacen("Change-Id: ", "", 1));
            break;
        }
    }
    change
}

#[tracing::instrument(level = tracing::Level::TRACE, skip(transaction))]
pub fn filter_commit(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    oid: git2::Oid,
    permissions: filter::Filter,
) -> JoshResult<git2::Oid> {
    let original_commit = {
        let obj = transaction.repo().find_object(oid, None)?;
        obj.peel_to_commit()?
    };

    let perms_commit = if let Some(s) = transaction.get_ref(permissions, oid) {
        s
    } else {
        tracing::trace!("apply_to_commit (permissions)");

        filter::apply_to_commit(permissions, &original_commit, transaction)?
    };

    if perms_commit != git2::Oid::zero() {
        let perms_commit = transaction.repo().find_commit(perms_commit)?;
        if !perms_commit.tree()?.is_empty() || perms_commit.parents().len() > 0 {
            tracing::event!(
                tracing::Level::WARN,
                msg = "filter_commit: missing permissions for commit",
                warn = true,
                oid = format!("{:?}", oid),
            );
            return Err(josh_error("missing permissions for commit"));
        }
    }

    let filter_commit = if let Some(s) = transaction.get_ref(filterobj, oid) {
        s
    } else {
        tracing::trace!("apply_to_commit");

        filter::apply_to_commit(filterobj, &original_commit, transaction)?
    };

    transaction.insert_ref(filterobj, oid, filter_commit);

    Ok(filter_commit)
}

pub fn filter_refs(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    refs: &[(String, git2::Oid)],
    permissions: filter::Filter,
) -> (Vec<(String, git2::Oid)>, Vec<(String, JoshError)>) {
    rs_tracing::trace_scoped!("filter_refs", "spec": filter::spec(filterobj));
    let s = tracing::Span::current();
    let _e = s.enter();
    let mut updated = vec![];
    let mut errors = vec![];

    tracing::trace!("filter_refs");

    for k in refs {
        let oid = match filter_commit(transaction, filterobj, k.1, permissions) {
            Ok(oid) => oid,
            Err(e) => {
                errors.push((k.0.to_string(), e));
                tracing::event!(
                    tracing::Level::WARN,
                    msg = "filter_refs: Can't filter reference",
                    warn = true,
                    from = k.0.as_str(),
                );
                git2::Oid::zero()
            }
        };
        updated.push((k.0.to_string(), oid));
    }

    (updated, errors)
}

pub fn update_refs(
    transaction: &cache::Transaction,
    updated: &mut Vec<(String, git2::Oid)>,
    headref: &str,
) {
    let mut head_oid = git2::Oid::zero();
    for (refname, oid) in updated.iter() {
        if refname == headref {
            head_oid = *oid;
        }
    }

    if !headref.is_empty() && head_oid == git2::Oid::zero() {
        updated.clear();
    }

    for (to_refname, filter_commit) in updated.iter() {
        if *filter_commit != git2::Oid::zero() {
            ok_or!(
                transaction
                    .repo()
                    .reference(to_refname, *filter_commit, true, "apply_filter")
                    .map(|_| ()),
                {
                    tracing::error!(
                        "can't update reference: {:?}, target: {:?}",
                        &to_refname,
                        filter_commit,
                    );
                }
            );
        }
    }
}

pub fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ std::path::Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        std::path::PathBuf::from(c.as_os_str())
    } else {
        std::path::PathBuf::new()
    };

    for component in components {
        match component {
            std::path::Component::Prefix(..) => unreachable!(),
            std::path::Component::RootDir => {
                ret.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                ret.pop();
            }
            std::path::Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

type Users = std::collections::HashMap<String, User>;

#[derive(Debug, serde::Deserialize)]
struct User {
    pub groups: Vec<String>,
}

type Groups = std::collections::HashMap<String, std::collections::HashMap<String, Group>>;
#[derive(Debug, serde::Deserialize)]
struct Group {
    pub whitelist: String,
    pub blacklist: String,
}

pub fn get_acl(
    users: &str,
    groups: &str,
    user: &str,
    repo: &str,
) -> JoshResult<(filter::Filter, filter::Filter)> {
    let users =
        std::fs::read_to_string(users).map_err(|_| josh_error("failed to read users file"))?;
    let users: Users = serde_yaml::from_str(&users)
        .map_err(|err| josh_error(format!("failed to parse users file: {}", err).as_str()))?;
    let groups =
        std::fs::read_to_string(groups).map_err(|_| josh_error("failed to read groups file"))?;
    let groups: Groups = serde_yaml::from_str(&groups)
        .map_err(|err| josh_error(format!("failed to parse groups file: {}", err).as_str()))?;

    return users
        .get(user)
        .and_then(|u| {
            let mut whitelist = filter::empty();
            let mut blacklist = filter::empty();
            for g in &u.groups {
                let lists = groups.get(repo).and_then(|repo| {
                    repo.get(g.as_str()).map(|group| {
                        let w = filter::parse(&group.whitelist);
                        let b = filter::parse(&group.blacklist);
                        (w, b)
                    })
                })?;
                if let Err(e) = lists.0 {
                    return Some(Err(JoshError(format!("Error parsing whitelist: {}", e))));
                }
                if let Err(e) = lists.1 {
                    return Some(Err(JoshError(format!("Error parsing blacklist: {}", e))));
                }
                if let Ok(w) = lists.0 {
                    whitelist = filter::compose(whitelist, w);
                }
                if let Ok(b) = lists.1 {
                    blacklist = filter::compose(blacklist, b);
                }
            }
            println!("w: {:?}, b: {:?}", whitelist, blacklist);
            Some(Ok((whitelist, blacklist)))
        })
        .unwrap_or_else(|| Ok((filter::empty(), filter::nop())));
}
