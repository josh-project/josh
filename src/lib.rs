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
    RejectMerge(usize),
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
    return percent_encoding::utf8_percent_encode(
        path.trim_matches('/'),
        FRAGMENT,
    )
    .to_string();
}

pub fn from_ns(path: &str) -> String {
    return percent_encoding::percent_decode_str(path.trim_matches('/'))
        .decode_utf8_lossy()
        .to_string();
}

pub fn to_filtered_ref(upstream_repo: &str, filter_spec: &str) -> String {
    return format!(
        "josh/filtered/{}/{}",
        to_ns(&upstream_repo),
        to_ns(&filter_spec)
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
        tracing::error!("JoshError: {:?}", item);
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
    for line in commit.message().unwrap_or("").split("\n") {
        if line.starts_with("Change-Id: ") {
            let id = line.replace("Change-Id: ", "");
            return Some(id);
        }
    }
    return None;
}

#[tracing::instrument(skip(transaction))]
fn filter_ref(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    from_refsname: &str,
    to_refname: &str,
) -> JoshResult<usize> {
    let mut updated_count = 0;
    if let Ok(reference) = transaction.repo().revparse_single(&from_refsname) {
        let oid = reference.id();
        let original_commit = transaction.repo().find_commit(oid)?;

        let filter_commit = if let Some(s) = transaction.get_ref(filterobj, oid)
        {
            s
        } else {
            tracing::trace!("apply_to_commit");

            filter::apply_to_commit(filterobj, &original_commit, &transaction)?
        };

        let previous = transaction
            .repo()
            .revparse_single(&to_refname)
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
                    .reference(&to_refname, filter_commit, true, "apply_filter")
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
    return Ok(updated_count);
}

#[tracing::instrument(skip(transaction))]
pub fn filter_refs(
    transaction: &cache::Transaction,
    filterobj: filter::Filter,
    refs: &[(String, String)],
) -> JoshResult<usize> {
    rs_tracing::trace_scoped!("filter_refs", "spec": filter::spec(filterobj));

    let mut updated_count = 0;
    for (k, v) in refs {
        updated_count += filter_ref(&transaction, filterobj, &k, &v)?;
    }
    return Ok(updated_count);
}

pub fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ std::path::Component::Prefix(..)) =
        components.peek().cloned()
    {
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
