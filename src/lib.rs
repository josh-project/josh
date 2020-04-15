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
extern crate pest_derive;

#[macro_use]
extern crate serde_json;

use tracing;

pub mod base_repo;
pub mod filters;
pub mod scratch;
pub mod shell;
pub mod view_maps;

pub use crate::filters::build_chain;
pub use crate::scratch::apply_view_to_refs;
pub use crate::scratch::unapply_view;

#[derive(Clone)]
pub enum UnapplyView {
    Done(git2::Oid),
    RejectMerge(usize),
    BranchDoesNotExist,
}

fn empty_tree_id() -> git2::Oid {
    return git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904")
        .unwrap();
}

fn empty_tree(repo: &git2::Repository) -> git2::Tree {
    repo.find_tree(empty_tree_id()).unwrap()
}

pub fn to_ns(path: &str) -> String {
    return path.trim_matches('/').replace("/", "/refs/namespaces/");
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
        josh_error("converted")
    }
}
