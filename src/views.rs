use super::replace_subtree;
use super::*;
use git2::*;
use pest::*;
use std::path::Path;
use std::path::PathBuf;

pub trait View {
    fn apply(&self, repo: &git2::Repository, tree: &git2::Tree) -> Option<git2::Oid>;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> Option<git2::Oid>;
}

struct NopView;

impl View for NopView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }
}

struct EmptyView;

impl View for EmptyView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        let empty = repo.treebuilder(None).unwrap().write().unwrap();
        Some(empty)
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }
}

struct ChainView {
    first: Box<dyn View>,
    second: Box<dyn View>,
}

impl View for ChainView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        if let Some(r) = self.first.apply(&repo, &tree) {
            if let Ok(t) = repo.find_tree(r) {
                return self.second.apply(&repo, &t);
            }
        }
        return None;
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
        let p = self.first.apply(&repo, &parent_tree).expect("no tree");
        let p = repo.find_tree(p).expect("no tree");
        let a = self.second.unapply(&repo, &tree, &p).expect("no tree");
        self.first
            .unapply(&repo, &repo.find_tree(a).expect("no tree"), &parent_tree)
    }
}

struct SubdirView {
    subdir: PathBuf,
}

impl View for SubdirView {
    fn apply(&self, _repo: &Repository, tree: &Tree) -> Option<Oid> {
        tree.get_path(&self.subdir).map(|x| x.id()).ok()
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
        Some(replace_subtree(&repo, &self.subdir, &tree, &parent_tree))
    }
}

struct PrefixView {
    prefix: PathBuf,
}

impl View for PrefixView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        let empty = repo
            .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        Some(replace_subtree(&repo, &self.prefix, &tree, &empty))
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Option<Oid> {
        tree.get_path(&self.prefix).map(|x| x.id()).ok()
    }
}

struct CombineView {
    base: Box<dyn View>,
    others: Vec<Box<dyn View>>,
    prefixes: Vec<PathBuf>,
}

impl View for CombineView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        let empty = repo.treebuilder(None).unwrap().write().unwrap();
        let mut base = some_or!(self.base.apply(&repo, &tree), { empty });

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {

            let otree = other.apply(&repo, &tree).unwrap_or(empty);
            let otree = repo.find_tree(otree).expect("can't find tree");
            base = replace_subtree(&repo, &prefix, &otree, &repo.find_tree(base).unwrap());
        }

        return Some(base);
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }
}

#[derive(Parser)]
#[grammar = "view_parser.pest"]
struct MyParser;

use pest::iterators::Pair;

fn make_view(cmd: &str, name: &str) -> Box<dyn View> {
    if cmd == "+" {
        return Box::new(PrefixView {
            prefix: Path::new(name).to_owned(),
        });
    } else if cmd == "empty" {
        println!("MKVIEW empty");
        return Box::new(EmptyView);
    } else {
        return Box::new(SubdirView {
            subdir: Path::new(name).to_owned(),
        });
    }
}

fn parse_item(pair: Pair<Rule>) -> Box<dyn View> {
    match pair.as_rule() {
        Rule::transform => {
            let mut inner = pair.into_inner();
            make_view(
                inner.next().unwrap().as_str(),
                inner.next().unwrap().as_str(),
            )
        }
        _ => unreachable!(),
    }
}

fn parse_file_entry(pair: Pair<Rule>, combine_view: &mut CombineView) {
    match pair.as_rule() {
        Rule::root_entry => {
            let mut inner = pair.into_inner();
            let v = inner.next().unwrap().as_str();
            println!("MKVIEW root_entry {:?}", v);
            combine_view.base = build_view(v);
        }
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let view = inner.next().unwrap().as_str();
            println!("MKVIEW file_entry {:?} {:?}", path, view);
            let view = build_view(view);
            combine_view.prefixes.push(Path::new(path).to_owned());
            combine_view.others.push(view);
        }
        _ => unreachable!(),
    }
}

pub fn build_view(viewstr: &str) -> Box<dyn View> {
    println!("MKVIEW {:?}", viewstr);
    if viewstr.starts_with("!") {
        let mut chain: Box<dyn View> = Box::new(NopView);
        if let Ok(r) = MyParser::parse(Rule::view, viewstr) {
            let mut r = r;
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                chain = Box::new(ChainView {
                    first: chain,
                    second: parse_item(pair),
                });
            }
            return chain;
        };
    }

    /* println!("MKVIEW {:?}", viewstr); */

    let mut combine_view = Box::new(CombineView {
        base: Box::new(NopView),
        others: vec![],
        prefixes: vec![],
    });

    if let Ok(r) = MyParser::parse(Rule::viewfile, viewstr) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            parse_file_entry(pair, &mut combine_view);
        }
    };

    return combine_view;
}
