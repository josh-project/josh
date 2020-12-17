/* #![deny(warnings)] */
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

pub mod filter_cache;
pub mod filters;
pub mod housekeeping;
pub mod query;
pub mod scratch;
pub mod shell;

pub use crate::filters::build_chain;
pub use crate::filters::overlay;
pub use crate::filters::parse;
pub use crate::filters::replace_subtree;
pub use crate::filters::substract;
pub use crate::scratch::apply_filter_to_refs;
pub use crate::scratch::unapply_filter;

#[derive(Clone)]
pub enum UnapplyFilter {
    Done(git2::Oid),
    RejectMerge(usize),
    BranchDoesNotExist,
}

fn empty_tree_id() -> git2::Oid {
    return git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")
        .unwrap();
}

pub fn empty_tree(repo: &git2::Repository) -> git2::Tree {
    repo.find_tree(empty_tree_id()).unwrap()
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

pub type JoshResult<T> = std::result::Result<T, JoshError>;

impl<T> std::convert::From<T> for JoshError
where
    T: std::error::Error,
{
    fn from(item: T) -> Self {
        tracing::error!("JoshError: {:?}", item);
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
