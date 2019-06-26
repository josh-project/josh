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
extern crate log;

#[macro_use]
extern crate pest_derive;

#[macro_use]
extern crate rs_tracing;

extern crate pest;

pub mod base_repo;
pub mod cgi;
pub mod scratch;
pub mod shell;
pub mod view_maps;
mod views;
pub mod virtual_repo;

pub use views::build_view;

#[derive(Clone)]
pub enum UnapplyView {
    Done(git2::Oid),
    RejectNoFF,
    RejectMerge,
    NoChanges,
}

fn empty_tree(repo: &git2::Repository) -> git2::Tree {
    return repo
        .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
        .unwrap();
}
