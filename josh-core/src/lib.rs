#![warn(unused_extern_crates)]

#[macro_export]
macro_rules! some_or {
    ($e:expr, $b:block) => {
        if let Some(x) = $e { x } else { $b }
    };
}

#[macro_export]
macro_rules! ok_or {
    ($e:expr, $b:block) => {
        if let Ok(x) = $e { x } else { $b }
    };
}

#[macro_use]
extern crate rs_tracing;

pub mod cache;
pub mod changes;
pub mod filter;
pub mod git;
pub mod history;
pub mod housekeeping;
pub mod link;
pub mod submodules;

#[derive(Debug)]
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
    type Error = anyhow::Error;
    fn try_from(s: String) -> anyhow::Result<Oid> {
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
    percent_encoding::utf8_percent_encode(path.trim_matches('/'), FRAGMENT).to_string()
}

pub fn from_ns(path: &str) -> String {
    percent_encoding::percent_decode_str(path.trim_matches('/'))
        .decode_utf8_lossy()
        .to_string()
}

pub fn to_filtered_ref(upstream_repo: &str, filter_spec: &str) -> String {
    format!(
        "josh/filtered/{}/{}",
        to_ns(upstream_repo),
        to_ns(filter_spec)
    )
}

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
        use std::sync::LazyLock;
        static REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new($re)
                .expect("can't compile regex")
        });
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
) -> anyhow::Result<git2::Oid> {
    let original_commit = {
        let obj = transaction.repo().find_object(oid, None)?;
        obj.peel_to_commit()?
    };

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
) -> (Vec<(String, git2::Oid)>, Vec<(String, anyhow::Error)>) {
    rs_tracing::trace_scoped!("filter_refs", "spec": filter::spec(filterobj));
    let s = tracing::Span::current();
    let _e = s.enter();
    let mut updated = vec![];
    let mut errors = vec![];

    tracing::trace!("filter_refs");

    for k in refs {
        let oid = match filter_commit(transaction, filterobj, k.1) {
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

pub fn update_refs(transaction: &cache::Transaction, updated: Vec<(String, git2::Oid)>) {
    for (refn, filtered_commit) in updated.into_iter() {
        if filtered_commit.is_zero() {
            continue;
        }

        if let Err(e) = transaction
            .repo()
            .reference(&refn, filtered_commit, true, "update_refs")
            .map(|_| ())
        {
            tracing::error!(
                error = %e,
                filtered_commit = %filtered_commit,
                refn = %refn,
                "can't update reference",
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
