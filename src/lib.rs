#![deny(warnings)]

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

extern crate git2;

#[macro_use]
extern crate pest_derive;

#[macro_use]
extern crate serde_json;

extern crate pest;
extern crate toml;
extern crate tracing;

pub mod base_repo;
pub mod cgi;
pub mod get_info;
pub mod scratch;
pub mod shell;
pub mod tomlq;
pub mod view_maps;
mod views;
pub mod virtual_repo;

pub use crate::get_info::get_info;
pub use crate::scratch::apply_view_to_refs;
pub use crate::scratch::unapply_view;
pub use crate::views::build_chain;
pub use crate::views::build_view;

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

pub fn to_ns(path: &str) -> String {
    return path.trim_matches('/').replace("/", "/refs/namespaces/");
}
