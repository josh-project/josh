use super::replace_subtree;
use super::*;
use git2::*;
use pest::*;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::str;

pub trait View {
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
    ) -> (git2::Oid, Vec<Oid>) {
        let full_tree = commit.tree().expect("commit has no tree");

        let mut parent_ids = vec![];
        for parent in commit.parents() {
            parent_ids.push(parent.id());
        }
        return self.apply_to_tree_and_parents(forward_maps, repo, (full_tree.id(), parent_ids));
    }

    fn apply_to_tree(&self, repo: &git2::Repository, tree: &git2::Tree) -> git2::Oid;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid;

    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>);

    fn viewstr(&self) -> String;
}

struct NopView;

fn default_apply_to_tree_and_parents(
    viewobj: &dyn View,
    forward_maps: &mut ViewMaps,
    repo: &git2::Repository,
    tree_and_parents: (git2::Oid, Vec<Oid>),
) -> (git2::Oid, Vec<Oid>) {
    let (tree, parents) = tree_and_parents;
    let mut transformed_parents_ids = vec![];
    for parent in parents {
        let p = apply_view_cached(repo, viewobj, parent, forward_maps, &mut ViewMap::new());
        if p != Oid::zero() {
            transformed_parents_ids.push(p);
        }
    }
    return (
        viewobj.apply_to_tree(&repo, &repo.find_tree(tree).unwrap()),
        transformed_parents_ids,
    );
}

impl View for NopView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        return default_apply_to_tree_and_parents(self, forward_maps, repo, tree_and_parents);
    }

    fn apply_to_tree(&self, _repo: &Repository, tree: &Tree) -> Oid {
        tree.id()
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Oid {
        tree.id()
    }

    fn viewstr(&self) -> String {
        return ":nop=nop".to_owned();
    }
}

struct EmptyView;

impl View for EmptyView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        return default_apply_to_tree_and_parents(self, forward_maps, repo, tree_and_parents);
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
    ) -> (git2::Oid, Vec<Oid>) {
        let full_tree = commit.tree().expect("commit has no tree");
        return (self.apply_to_tree(&repo, &full_tree), vec![]);
    }
    fn apply_to_tree(&self, repo: &Repository, _tree: &Tree) -> Oid {
        empty_tree(repo).id()
    }

    fn unapply(&self, _repo: &Repository, _tree: &Tree, parent_tree: &Tree) -> Oid {
        parent_tree.id()
    }

    fn viewstr(&self) -> String {
        return ":empty=empty".to_owned();
    }
}

struct ChainView {
    first: Box<dyn View>,
    second: Box<dyn View>,
}

impl View for ChainView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        let r = self
            .first
            .apply_to_tree_and_parents(forward_maps, repo, tree_and_parents);
        return self.second.apply_to_tree_and_parents(forward_maps, repo, r);
    }

    fn apply_to_tree(&self, repo: &Repository, tree: &Tree) -> Oid {
        let r = self.first.apply_to_tree(&repo, &tree);
        if let Ok(t) = repo.find_tree(r) {
            return self.second.apply_to_tree(&repo, &t);
        }
        return repo.treebuilder(None).unwrap().write().unwrap();
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Oid {
        let p = self.first.apply_to_tree(&repo, &parent_tree);
        let p = repo.find_tree(p).expect("no tree");
        let a = self.second.unapply(&repo, &tree, &p);
        self.first
            .unapply(&repo, &repo.find_tree(a).expect("no tree"), &parent_tree)
    }

    fn viewstr(&self) -> String {
        return format!("{}{}", &self.first.viewstr(), &self.second.viewstr());
    }
}

struct SubdirView {
    subdir: PathBuf,
}

impl View for SubdirView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        return default_apply_to_tree_and_parents(self, forward_maps, repo, tree_and_parents);
    }
    fn apply_to_tree(&self, repo: &Repository, tree: &Tree) -> Oid {
        return tree
            .get_path(&self.subdir)
            .map(|x| x.id())
            .unwrap_or(empty_tree(repo).id());
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Oid {
        replace_subtree(&repo, &self.subdir, &tree, &parent_tree)
    }

    fn viewstr(&self) -> String {
        return format!(":/{}", &self.subdir.to_str().unwrap());
    }
}

struct PrefixView {
    prefix: PathBuf,
}

impl View for PrefixView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        return default_apply_to_tree_and_parents(self, forward_maps, repo, tree_and_parents);
    }
    fn apply_to_tree(&self, repo: &Repository, tree: &Tree) -> Oid {
        replace_subtree(&repo, &self.prefix, &tree, &empty_tree(repo))
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Oid {
        return tree
            .get_path(&self.prefix)
            .map(|x| x.id())
            .unwrap_or(empty_tree(repo).id());
    }

    fn viewstr(&self) -> String {
        return format!(":prefix={}", &self.prefix.to_str().unwrap());
    }
}

struct CombineView {
    base: Box<dyn View>,
    others: Vec<Box<dyn View>>,
    prefixes: Vec<PathBuf>,
}

impl View for CombineView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        return default_apply_to_tree_and_parents(self, forward_maps, repo, tree_and_parents);
    }

    fn apply_to_tree(&self, repo: &Repository, tree: &Tree) -> Oid {
        let mut base = self.base.apply_to_tree(&repo, &tree);

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            let otree = other.apply_to_tree(&repo, &tree);
            if otree == empty_tree(repo).id() {
                continue;
            }
            let otree = repo.find_tree(otree).expect("can't find tree");
            base = replace_subtree(&repo, &prefix, &otree, &repo.find_tree(base).unwrap());
        }

        return base;
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Oid {
        let mut base_wo = tree.id();

        for prefix in self.prefixes.iter() {
            base_wo = replace_subtree(
                repo,
                prefix,
                &empty_tree(repo),
                &repo.find_tree(base_wo).unwrap(),
            );
        }

        let mut res = self
            .base
            .unapply(repo, &repo.find_tree(base_wo).unwrap(), parent_tree);

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            let r = ok_or!(tree.get_path(&prefix).map(|x| x.id()), {
                continue;
            });
            let r = repo.find_tree(r).unwrap();
            let ua = other.unapply(&repo, &r, &parent_tree);

            let merged = repo
                .merge_trees(
                    &parent_tree,
                    &repo.find_tree(res).unwrap(),
                    &repo.find_tree(ua).unwrap(),
                    Some(MergeOptions::new().file_favor(FileFavor::Theirs)),
                )
                .unwrap()
                .write_tree_to(&repo)
                .unwrap();

            res = merged;
        }

        return res;
    }

    fn viewstr(&self) -> String {
        let mut s = format!("/ = {}", &self.base.viewstr());

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            s = format!("{}\n{} = {}", &s, prefix.to_str().unwrap(), other.viewstr());
        }
        return s;
    }
}

struct WorkspaceView {
    ws_path: PathBuf,
}

fn combine_view_from_ws(repo: &Repository, tree: &Tree, ws_path: &Path) -> Box<CombineView> {
    let base = Box::new(SubdirView {
        subdir: ws_path.to_owned(),
    });
    let wsp = ws_path.join("workspace.josh");
    let ws_config_oid = ok_or!(tree.get_path(&wsp).map(|x| x.id()), {
        return build_combine_view("", base);
    });

    let ws_blob = ok_or!(repo.find_blob(ws_config_oid), {
        return build_combine_view("", base);
    });

    let ws_content = ok_or!(str::from_utf8(ws_blob.content()), {
        return build_combine_view("", base);
    });

    return build_combine_view(ws_content, base);
}

impl View for WorkspaceView {
    fn apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<Oid>),
    ) -> (git2::Oid, Vec<Oid>) {
        let (tree, parents) = tree_and_parents;
        let full_tree = repo.find_tree(tree).unwrap();

        let mut in_this = HashSet::new();

        let cw = combine_view_from_ws(repo, &full_tree, &self.ws_path);

        for (other, prefix) in cw.others.iter().zip(cw.prefixes.iter()) {
            in_this.insert(format!(
                "{} = {}",
                prefix.to_str().unwrap(),
                other.viewstr()
            ));
        }

        let mut transformed_parents_ids = vec![];
        for parent in parents.iter() {
            let p = apply_view_cached(repo, self, *parent, forward_maps, &mut ViewMap::new());
            if p != Oid::zero() {
                transformed_parents_ids.push(p);
            }

            let parent_commit = repo.find_commit(*parent).unwrap();

            let pcw = combine_view_from_ws(repo, &parent_commit.tree().unwrap(), &self.ws_path);

            for (other, prefix) in pcw.others.iter().zip(pcw.prefixes.iter()) {
                in_this.remove(&format!(
                    "{} = {}",
                    prefix.to_str().unwrap(),
                    other.viewstr()
                ));
            }
        }

        let mut s = String::new();
        for x in in_this {
            s = format!("{}{}\n", s, x);
        }

        let pcw: Box<dyn View> = build_combine_view(&s, Box::new(EmptyView));

        for parent in parents {
            let p = apply_view_cached(repo, &*pcw, parent, forward_maps, &mut ViewMap::new());
            if p != Oid::zero() {
                transformed_parents_ids.push(p);
            }
            break;
        }

        return (cw.apply_to_tree(repo, &full_tree), transformed_parents_ids);
    }

    fn apply_to_tree(&self, repo: &Repository, tree: &Tree) -> Oid {
        return combine_view_from_ws(repo, tree, &self.ws_path).apply_to_tree(repo, tree);
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Oid {
        /* let mut cw = combine_view_from_ws(repo, parent_tree, &self.ws_path); */
        let mut cw = combine_view_from_ws(repo, tree, &PathBuf::from(""));

        cw.base = Box::new(SubdirView {
            subdir: self.ws_path.to_owned(),
        });
        return cw.unapply(repo, tree, parent_tree);
    }

    fn viewstr(&self) -> String {
        return format!(":workspace={}", &self.ws_path.to_str().unwrap());
    }
}

#[derive(Parser)]
#[grammar = "view_parser.pest"]
struct MyParser;

use pest::iterators::Pair;

fn make_view(cmd: &str, name: &str) -> Box<dyn View> {
    if cmd == "+" || cmd == "prefix" {
        return Box::new(PrefixView {
            prefix: Path::new(name).to_owned(),
        });
    } else if cmd == "empty" {
        println!("MKVIEW empty");
        return Box::new(EmptyView);
    } else if cmd == "nop" {
        println!("MKVIEW nop");
        return Box::new(NopView);
    } else if cmd == "workspace" {
        println!("MKVIEW workspace");
        return Box::new(WorkspaceView {
            ws_path: Path::new(name).to_owned(),
        });
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

fn build_combine_view(viewstr: &str, base: Box<dyn View>) -> Box<CombineView> {
    let mut combine_view = Box::new(CombineView {
        base: base,
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

pub fn build_view(viewstr: &str) -> Box<dyn View> {
    println!("MKVIEW {:?}", viewstr);

    if viewstr.starts_with("!") || viewstr.starts_with(":") {
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

    return build_combine_view(viewstr, Box::new(EmptyView));
}
