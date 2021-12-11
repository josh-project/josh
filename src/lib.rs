#![deny(warnings)]
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

#[macro_use]
extern crate handlebars;

#[macro_use]
extern crate pest_derive;

#[macro_use]
extern crate serde_json;

use std::collections::HashMap;
use tracing;

pub mod cache;
pub mod filter;
pub mod graphql;
pub mod history;
pub mod housekeeping;
pub mod query;
pub mod shell;

#[derive(Clone)]
pub enum UnapplyResult {
    Done(git2::Oid),
    RejectMerge(String),
    RejectAmend(String),
    BranchDoesNotExist,
}

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
    return format!(
        "josh/filtered/{}/{}",
        to_ns(upstream_repo),
        to_ns(filter_spec)
    );
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

pub type JoshResult<T> = std::result::Result<T, JoshError>;

impl<T> std::convert::From<T> for JoshError
where
    T: std::error::Error,
{
    fn from(item: T) -> Self {
        tracing::event!(tracing::Level::ERROR, item = ?item, error = true);
        log::error!("JoshError: {:?}", item);
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

lazy_static! {
    static ref REGEX: regex::Regex =
        regex::Regex::new($re)
            .expect("can't compile regex");
}

        let caps = if let Some(caps) = REGEX.captures(&path) {
            caps
        } else {
            return None;
        };

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

pub fn get_change_id(commit: &git2::Commit) -> Option<String> {
    for line in commit.message().unwrap_or("").split('\n') {
        if line.starts_with("Change-Id: ") {
            let id = line.replace("Change-Id: ", "");
            return Some(id);
        }
    }
    None
}

#[tracing::instrument(skip(transaction))]
pub fn filter_ref(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    from_refsname: &str,
    to_refname: &str,
    permissions: filter::Filter,
) -> JoshResult<usize> {
    let mut updated_count = 0;
    if let Ok(reference) = transaction.repo().revparse_single(from_refsname) {
        let original_commit = reference.peel_to_commit()?;
        let oid = original_commit.id();

        let perms_commit = if let Some(s) = transaction.get_ref(permissions, oid) {
            s
        } else {
            tracing::trace!("apply_to_commit (permissions)");

            filter::apply_to_commit(permissions, &original_commit, &transaction)?
        };

        if perms_commit != git2::Oid::zero() {
            let perms_commit = transaction.repo().find_commit(perms_commit)?;
            if !perms_commit.tree()?.is_empty() || perms_commit.parents().len() > 0 {
                tracing::event!(
                    tracing::Level::WARN,
                    msg = "filter_refs: missing permissions for ref",
                    warn = true,
                    reference = from_refsname,
                );
                return Err(josh_error("missing permissions for ref"));
            }
        }

        let filter_commit = if let Some(s) = transaction.get_ref(filterobj, oid) {
            s
        } else {
            tracing::trace!("apply_to_commit");

            filter::apply_to_commit(filterobj, &original_commit, transaction)?
        };

        let previous = transaction
            .repo()
            .revparse_single(to_refname)
            .map(|x| x.id())
            .unwrap_or(git2::Oid::zero());

        if filter_commit != previous {
            updated_count += 1;
            tracing::trace!(
                "filter_ref: update reference: {:?} -> {:?}, target: {:?}, filter: {:?}",
                &from_refsname,
                &to_refname,
                filter_commit,
                &filter::spec(filterobj),
            );
        }

        transaction.insert_ref(filterobj, oid, filter_commit);

        if filter_commit != git2::Oid::zero() {
            ok_or!(
                transaction
                    .repo()
                    .reference(to_refname, filter_commit, true, "apply_filter")
                    .map(|_| ()),
                {
                    tracing::error!(
                        "can't update reference: {:?} -> {:?}, target: {:?}, filter: {:?}",
                        &from_refsname,
                        &to_refname,
                        filter_commit,
                        &filter::spec(filterobj),
                    );
                }
            );
        }
    } else {
        tracing::warn!("filter_ref: Can't find reference {:?}", &from_refsname);
    };
    Ok(updated_count)
}

pub fn filter_refs(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    refs: &[(String, String)],
    permissions: filter::Filter,
) -> JoshResult<usize> {
    rs_tracing::trace_scoped!("filter_refs", "spec": filter::spec(filterobj));
    let s = tracing::Span::current();
    let _e = s.enter();

    tracing::trace!("filter_refs");

    let mut updated_count = 0;
    for (k, v) in refs {
        updated_count += ok_or!(filter_ref(&transaction, filterobj, &k, &v, permissions), {
            tracing::event!(
                tracing::Level::WARN,
                msg = "filter_refs: Can't filter reference",
                warn = true,
                from = k.as_str(),
                to = v.as_str()
            );
            0
        });
    }
    Ok(updated_count)
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

type Users = HashMap<String, User>;

#[derive(Debug, serde::Deserialize)]
struct User {
    pub groups: toml::value::Array,
}

type Groups = HashMap<String, HashMap<String, Group>>;
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
                    repo.get(g.as_str()?).and_then(|group| {
                        let w = filter::parse(&group.whitelist);
                        let b = filter::parse(&group.blacklist);
                        Some((w, b))
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
        .unwrap_or(Ok((filter::empty(), filter::nop())));
}
