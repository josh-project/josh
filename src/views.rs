use super::empty_tree_id;
use super::scratch;
use super::view_maps::ViewMaps;
use pest::iterators::Pair;
use pest::Parser;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::str;

fn filter_parents<'a>(
    original_commit: &'a git2::Commit,
    new_tree: git2::Oid,
    transformed_parent_refs: Vec<&'a git2::Commit>,
) -> Vec<&'a git2::Commit<'a>> {
    let affects_transformed = transformed_parent_refs
        .iter()
        .any(|x| new_tree != x.tree_id());

    let all_diffs_empty = original_commit
        .parents()
        .all(|x| x.tree_id() == original_commit.tree_id());

    return if affects_transformed || all_diffs_empty {
        transformed_parent_refs
    } else {
        vec![]
    };
}

pub trait View {
    fn apply_view_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> git2::Oid {

        if forward_maps.has(&repo, &self.viewstr(), commit.id()) {
            return forward_maps.get(&self.viewstr(), commit.id());
        }
        let new_tree = self.apply_to_tree(&repo, &commit.tree().unwrap(), commit.id());

        let parents = self.transform_parents(repo, commit, forward_maps, backward_maps);

        let transformed_parents: Vec<_> = parents
            .iter()
            .filter(|x| **x != git2::Oid::zero())
            .map(|x| repo.find_commit(*x).unwrap())
            .collect();

        if new_tree == empty_tree_id()
            && commit.tree_id() != empty_tree_id()
            && transformed_parents.len() != 0
        {
            return transformed_parents[0].id();
        }
        if new_tree == empty_tree_id() {
            return git2::Oid::zero();
        }

        let filtered_transformed_parent_refs: Vec<&_> =
            filter_parents(&commit, new_tree, transformed_parents.iter().collect());

        if filtered_transformed_parent_refs.len() == 0 && transformed_parents.len() != 0 {
            return transformed_parents[0].id();
        }

        let new_tree = repo
            .find_tree(new_tree)
            .expect("apply_view_to_commit: can't find tree");

        return scratch::rewrite(&repo, &commit, &filtered_transformed_parent_refs, &new_tree);
    }

    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid>;

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> git2::Oid;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid;

    fn prefixes(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn viewstr(&self) -> String;
}

struct NopView;

impl View for NopView {
    fn transform_parents(
        &self,
        _repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit.parent_ids().collect();
    }
    fn apply_view_to_commit(
        &self,
        _repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> git2::Oid {
        return commit.id();
    }

    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        tree.id()
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> git2::Oid {
        tree.id()
    }

    fn viewstr(&self) -> String {
        return ":nop=nop".to_owned();
    }
}

struct EmptyView;

impl View for EmptyView {
    fn transform_parents(
        &self,
        _repo: &git2::Repository,
        _commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return vec![];
    }
    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        empty_tree_id()
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        parent_tree.id()
    }

    fn viewstr(&self) -> String {
        return ":empty=empty".to_owned();
    }
}

struct CutoffView {
    rev: git2::Oid,
}

impl View for CutoffView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn apply_view_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> git2::Oid {
        /* if commit_id == self.rev { */
        /*     return (tp.0, vec![]); */
        /* } */
        return scratch::rewrite(&repo, &commit, &vec![], &commit.tree().unwrap());
    }

    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        tree.id()
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> git2::Oid {
        tree.id()
    }

    fn viewstr(&self) -> String {
        return format!(":cutoff={}", self.rev);
    }
}

struct ChainView {
    first: Box<dyn View>,
    second: Box<dyn View>,
}

impl View for ChainView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn apply_view_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> git2::Oid {
        let r = self
            .first
            .apply_view_to_commit(repo, commit, forward_maps, backward_maps, _meta);
        return self.second.apply_view_to_commit(
            repo,
            &repo.find_commit(r).unwrap(),
            forward_maps,
            backward_maps,
            _meta,
        );
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> git2::Oid {
        let r = self.first.apply_to_tree(&repo, &tree, commit_id);
        if let Ok(t) = repo.find_tree(r) {
            return self.second.apply_to_tree(&repo, &t, commit_id);
        }
        return repo.treebuilder(None).unwrap().write().unwrap();
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        let p = self
            .first
            .apply_to_tree(&repo, &parent_tree, git2::Oid::zero());
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
    path: PathBuf,
}

impl SubdirView {
    fn new(path: &Path) -> Box<dyn View> {
        let mut components = path.iter();
        let mut chain: Box<dyn View> = if let Some(comp) = components.next() {
            Box::new(SubdirView {
                path: Path::new(comp).to_owned(),
            })
        } else {
            Box::new(NopView)
        };

        for comp in components {
            chain = Box::new(ChainView {
                first: chain,
                second: Box::new(SubdirView {
                    path: Path::new(comp).to_owned(),
                }),
            })
        }
        return chain;
    }
}

impl View for SubdirView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        return tree
            .get_path(&self.path)
            .map(|x| x.id())
            .unwrap_or(empty_tree_id());
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        replace_subtree(&repo, &self.path, tree.id(), &parent_tree)
    }

    fn viewstr(&self) -> String {
        return format!(":/{}", &self.path.to_str().unwrap());
    }
}

struct PrefixView {
    prefix: PathBuf,
}

impl View for PrefixView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        replace_subtree(
            &repo,
            &self.prefix,
            tree.id(),
            &repo.find_tree(empty_tree_id()).unwrap(),
        )
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> git2::Oid {
        return tree
            .get_path(&self.prefix)
            .map(|x| x.id())
            .unwrap_or(empty_tree_id());
    }

    fn viewstr(&self) -> String {
        return format!(":prefix={}", &self.prefix.to_str().unwrap());
    }
}

struct InfoFileView {
    values: BTreeMap<String, String>,
}

impl View for InfoFileView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> git2::Oid {
        let mut s = "".to_owned();
        for (k, v) in self.values.iter() {
            let v = v.replace("<colon>", ":").replace("<comma>", ",");
            if k == "prefix" {
                continue;
            }
            s = format!(
                "{}{}: {}\n",
                &s,
                k,
                match v.as_str() {
                    "#sha1" => commit_id.to_string(),
                    "#tree" => tree
                        .get_path(&Path::new(&self.values["prefix"]))
                        .map(|x| x.id())
                        .unwrap_or(git2::Oid::zero())
                        .to_string(),
                    _ => v,
                }
            );
        }
        replace_subtree(
            repo,
            &Path::new(&self.values["prefix"]).join(".joshinfo"),
            repo.blob(s.as_bytes()).unwrap(),
            &tree,
        )
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> git2::Oid {
        tree.id()
    }

    fn viewstr(&self) -> String {
        let mut s = format!(":info=");

        for (k, v) in self.values.iter() {
            s = format!("{}{}={},", s, k, v);
        }
        return s.trim_end_matches(",").to_string();
    }
}

struct CombineView {
    base: Box<dyn View>,
    others: Vec<Box<dyn View>>,
    prefixes: Vec<PathBuf>,
}

impl View for CombineView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn prefixes(&self) -> HashMap<String, String> {
        let mut p = HashMap::new();
        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            p.insert(prefix.to_str().unwrap().to_owned(), other.viewstr());
        }
        p
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> git2::Oid {
        let mut base = self.base.apply_to_tree(&repo, &tree, commit_id);

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            let otree = other.apply_to_tree(&repo, &tree, commit_id);
            if otree == empty_tree_id() {
                continue;
            }
            let otree = repo.find_tree(otree).expect("can't find tree");
            base = replace_subtree(&repo, &prefix, otree.id(), &repo.find_tree(base).unwrap());
        }

        return base;
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        let mut base_wo = tree.id();

        for prefix in self.prefixes.iter() {
            base_wo = replace_subtree(
                repo,
                prefix,
                empty_tree_id(),
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
                    Some(git2::MergeOptions::new().file_favor(git2::FileFavor::Theirs)),
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

fn combine_view_from_ws(
    repo: &git2::Repository,
    tree: &git2::Tree,
    ws_path: &Path,
) -> Box<CombineView> {
    let base = SubdirView::new(&ws_path);
    let wsp = ws_path.join("workspace.josh");
    let ws_config_oid = ok_or!(tree.get_path(&wsp).map(|x| x.id()), {
        return build_combine_view(repo, "", base);
    });

    let ws_blob = ok_or!(repo.find_blob(ws_config_oid), {
        return build_combine_view(repo, "", base);
    });

    let ws_content = ok_or!(str::from_utf8(ws_blob.content()), {
        return build_combine_view(repo, "", base);
    });

    return build_combine_view(repo, ws_content, base);
}

impl WorkspaceView {
    fn ws_apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<git2::Oid>),
        commit_id: git2::Oid,
    ) -> (git2::Oid, Vec<git2::Oid>) {
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
            let p = scratch::apply_view_cached(repo, self, *parent, forward_maps, backward_maps);
            if p != git2::Oid::zero() {
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

        let pcw: Box<dyn View> = build_combine_view(repo, &s, Box::new(EmptyView));

        for parent in parents {
            let p = scratch::apply_view_cached(repo, &*pcw, parent, forward_maps, backward_maps);
            if p != git2::Oid::zero() {
                transformed_parents_ids.push(p);
            }
            break;
        }

        return (
            cw.apply_to_tree(repo, &full_tree, commit_id),
            transformed_parents_ids,
        );
    }
}

impl View for WorkspaceView {
    fn transform_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> Vec<git2::Oid> {
        return commit
            .parents()
            .map(|x| scratch::apply_view_cached(repo, self, x.id(), forward_maps, backward_maps))
            .collect();
    }
    fn apply_view_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> git2::Oid {
        if forward_maps.has(repo, &self.viewstr(), commit.id()) {
            return forward_maps.get(&self.viewstr(), commit.id());
        }

        let (new_tree, transformed_parents_ids) = self.ws_apply_to_tree_and_parents(
            forward_maps,
            backward_maps,
            repo,
            (commit.tree_id(), commit.parents().map(|x| x.id()).collect()),
            commit.id(),
        );

        if new_tree == empty_tree_id() {
            return git2::Oid::zero();
        }

        let transformed_parents: Vec<_> = transformed_parents_ids
            .iter()
            .map(|x| repo.find_commit(*x).unwrap())
            .collect();

        let filtered_transformed_parent_refs: Vec<&git2::Commit> =
            filter_parents(&commit, new_tree, transformed_parents.iter().collect());

        if filtered_transformed_parent_refs.len() == 0 && transformed_parents.len() != 0 {
            return transformed_parents[0].id();
        }

        let new_tree = repo
            .find_tree(new_tree)
            .expect("apply_view_to_commit: can't find tree");

        return scratch::rewrite(&repo, &commit, &filtered_transformed_parent_refs, &new_tree);
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> git2::Oid {
        return combine_view_from_ws(repo, tree, &self.ws_path)
            .apply_to_tree(repo, tree, commit_id);
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        let mut cw = combine_view_from_ws(repo, tree, &PathBuf::from(""));

        cw.base = SubdirView::new(&self.ws_path);
        return cw.unapply(repo, tree, parent_tree);
    }

    fn viewstr(&self) -> String {
        return format!(":workspace={}", &self.ws_path.to_str().unwrap());
    }
}

#[derive(Parser)]
#[grammar = "view_parser.pest"]
struct MyParser;

fn make_view(repo: &git2::Repository, cmd: &str, name: &str) -> Box<dyn View> {
    if cmd == "+" || cmd == "prefix" {
        return Box::new(PrefixView {
            prefix: Path::new(name).to_owned(),
        });
    } else if cmd == "empty" {
        return Box::new(EmptyView);
    } else if cmd == "info" {
        let mut s = BTreeMap::new();
        for p in name.split(",") {
            let x: Vec<String> = p.split("=").map(|x| x.to_owned()).collect();
            if x.len() == 2 {
                s.insert(x[0].to_owned(), x[1].to_owned());
            } else {
                s.insert("prefix".to_owned(), x[0].to_owned());
            }
        }
        return Box::new(InfoFileView { values: s });
    } else if cmd == "nop" {
        return Box::new(NopView);
    } else if cmd == "cutoff" {
        return Box::new(CutoffView {
            rev: ok_or!(
                repo.revparse_single(&name)
                    .and_then(|r| r.peel_to_commit())
                    .map(|r| r.id()),
                {
                    return Box::new(EmptyView);
                }
            ),
        });
    } else if cmd == "workspace" {
        return Box::new(WorkspaceView {
            ws_path: Path::new(name).to_owned(),
        });
    } else {
        return SubdirView::new(&Path::new(name));
    }
}

fn parse_item(repo: &git2::Repository, pair: Pair<Rule>) -> Box<dyn View> {
    match pair.as_rule() {
        Rule::transform => {
            let mut inner = pair.into_inner();
            make_view(
                repo,
                inner.next().unwrap().as_str(),
                inner.next().unwrap().as_str(),
            )
        }
        _ => unreachable!(),
    }
}

fn parse_file_entry(repo: &git2::Repository, pair: Pair<Rule>, combine_view: &mut CombineView) {
    match pair.as_rule() {
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let view = inner.next().unwrap().as_str();
            let view = build_view(repo, view);
            combine_view.prefixes.push(Path::new(path).to_owned());
            combine_view.others.push(view);
        }
        _ => unreachable!(),
    }
}

fn build_combine_view(
    repo: &git2::Repository,
    viewstr: &str,
    base: Box<dyn View>,
) -> Box<CombineView> {
    let mut combine_view = Box::new(CombineView {
        base: base,
        others: vec![],
        prefixes: vec![],
    });

    if let Ok(r) = MyParser::parse(Rule::viewfile, viewstr) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            parse_file_entry(repo, pair, &mut combine_view);
        }
    };

    return combine_view;
}

pub fn build_chain(first: Box<dyn View>, second: Box<dyn View>) -> Box<dyn View> {
    Box::new(ChainView {
        first: first,
        second: second,
    })
}

pub fn build_view(repo: &git2::Repository, viewstr: &str) -> Box<dyn View> {
    if viewstr.starts_with("!") || viewstr.starts_with(":") {
        let mut chain: Option<Box<dyn View>> = None;
        if let Ok(r) = MyParser::parse(Rule::view, viewstr) {
            let mut r = r;
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                let v = parse_item(repo, pair);
                chain = Some(if let Some(c) = chain {
                    Box::new(ChainView {
                        first: c,
                        second: v,
                    })
                } else {
                    v
                });
            }
            return chain.unwrap_or(Box::new(NopView));
        };
    }

    return build_combine_view(repo, viewstr, Box::new(EmptyView));
}

fn get_subtree(tree: &git2::Tree, path: &Path) -> Option<git2::Oid> {
    tree.get_path(path).map(|x| x.id()).ok()
}

fn replace_child(
    repo: &git2::Repository,
    child: &Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> git2::Oid {
    let mode = if let Ok(_) = repo.find_tree(oid) {
        0o0040000 // GIT_FILEMODE_TREE
    } else {
        0o0100644
    };

    let full_tree_id = {
        let mut builder = repo
            .treebuilder(Some(&full_tree))
            .expect("replace_child: can't create treebuilder");
        builder
            .insert(child, oid, mode)
            .expect("replace_child: can't insert tree");
        builder.write().expect("replace_child: can't write tree")
    };
    return full_tree_id;
}

fn replace_subtree(
    repo: &git2::Repository,
    path: &Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> git2::Oid {
    if path.components().count() == 1 {
        return repo
            .find_tree(replace_child(&repo, path, oid, full_tree))
            .expect("replace_child: can't find new tree")
            .id();
    } else {
        let name = Path::new(path.file_name().expect("no module name"));
        let path = path.parent().expect("module not in subdir");

        let st = if let Some(st) = get_subtree(&full_tree, path) {
            repo.find_tree(st).unwrap()
        } else {
            let empty = repo.treebuilder(None).unwrap().write().unwrap();
            repo.find_tree(empty).unwrap()
        };

        let tree = repo
            .find_tree(replace_child(&repo, name, oid, &st))
            .expect("replace_child: can't find new tree");

        return replace_subtree(&repo, path, tree.id(), full_tree);
    }
}
