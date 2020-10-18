use super::empty_tree;
use super::empty_tree_id;
use super::scratch;
use super::view_maps;
use super::view_maps::ViewMaps;
use pest::Parser;
use std::collections::HashMap;
use std::path::Path;

fn select_parent_commits<'a>(
    original_commit: &'a git2::Commit,
    filtered_tree_id: git2::Oid,
    filtered_parent_commits: Vec<&'a git2::Commit>,
) -> Vec<&'a git2::Commit<'a>> {
    let affects_filtered = filtered_parent_commits
        .iter()
        .any(|x| filtered_tree_id != x.tree_id());

    let all_diffs_empty = original_commit
        .parents()
        .all(|x| x.tree_id() == original_commit.tree_id());

    return if affects_filtered || all_diffs_empty {
        filtered_parent_commits
    } else {
        vec![]
    };
}

fn create_filtered_commit(
    repo: &git2::Repository,
    original_commmit: &git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: &git2::Tree,
) -> super::JoshResult<git2::Oid> {
    let filtered_parent_commits: std::result::Result<Vec<_>, _> =
        filtered_parent_ids
            .iter()
            .filter(|x| **x != git2::Oid::zero())
            .map(|x| repo.find_commit(*x))
            .collect();

    let filtered_parent_commits = filtered_parent_commits?;

    let selected_filtered_parent_commits: Vec<&_> = select_parent_commits(
        &original_commmit,
        filtered_tree.id(),
        filtered_parent_commits.iter().collect(),
    );

    if selected_filtered_parent_commits.len() == 0 {
        if filtered_parent_commits.len() != 0 {
            return Ok(filtered_parent_commits[0].id());
        }
        if filtered_tree.id() == empty_tree_id() {
            return Ok(git2::Oid::zero());
        }
    }

    return scratch::rewrite(
        &repo,
        &original_commmit,
        &selected_filtered_parent_commits,
        &filtered_tree,
    );
}

pub trait Filter {
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(&repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }
        let filtered_tree =
            self.apply_to_tree(&repo, &commit.tree()?, commit.id());

        let filtered_parent_ids =
            self.apply_to_parents(repo, commit, forward_maps, backward_maps)?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            &find_tree_or_error(
                &repo,
                filtered_tree,
                Some(&commit),
                &self.filter_spec(),
            ),
        );
    }

    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>>;

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

    fn filter_spec(&self) -> String;
}

struct NopView;

impl Filter for NopView {
    fn apply_to_parents(
        &self,
        _repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return Ok(commit.parent_ids().collect());
    }
    fn apply_to_commit(
        &self,
        _repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        return Ok(commit.id());
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

    fn filter_spec(&self) -> String {
        return ":nop=nop".to_owned();
    }
}

struct EmptyView;

impl Filter for EmptyView {
    fn apply_to_commit(
        &self,
        _repo: &git2::Repository,
        _commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        return Ok(git2::Oid::zero());
    }
    fn apply_to_parents(
        &self,
        _repo: &git2::Repository,
        _commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return Ok(vec![]);
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

    fn filter_spec(&self) -> String {
        return ":empty=empty".to_owned();
    }
}

struct CutoffView {
    /* rev: git2::Oid, */
    name: String,
    /*         rev: ok_or!( */
    /*             repo.revparse_single(&name) */
    /*                 .and_then(|r| r.peel_to_commit()) */
    /*                 .map(|r| r.id()), */
    /*             { */
    /*                 return Box::new(EmptyView); */
    /*             } */
    /*         ), */
}

impl Filter for CutoffView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }

    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut ViewMaps,
        _backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        return scratch::rewrite(&repo, &commit, &vec![], &commit.tree()?);
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

    fn filter_spec(&self) -> String {
        return format!(":cutoff={}", self.name);
    }
}

struct ChainView {
    first: Box<dyn Filter>,
    second: Box<dyn Filter>,
}

impl Filter for ChainView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        let r = self.first.apply_to_commit(
            repo,
            commit,
            forward_maps,
            backward_maps,
            _meta,
        )?;

        let commit = ok_or!(repo.find_commit(r), {
            return Ok(git2::Oid::zero());
        });
        return self.second.apply_to_commit(
            repo,
            &commit,
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
        return empty_tree_id();
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        let p =
            self.first
                .apply_to_tree(&repo, &parent_tree, git2::Oid::zero());
        let p = repo.find_tree(p).expect("no tree");
        let a = self.second.unapply(&repo, &tree, &p);
        self.first.unapply(
            &repo,
            &repo.find_tree(a).expect("no tree"),
            &parent_tree,
        )
    }

    fn filter_spec(&self) -> String {
        return format!(
            "{}{}",
            &self.first.filter_spec(),
            &self.second.filter_spec()
        )
        .replacen(":nop=nop", "", 1);
    }
}

struct SubdirView {
    path: std::path::PathBuf,
}

impl SubdirView {
    fn new(path: &Path) -> Box<dyn Filter> {
        let mut components = path.iter();
        let mut chain: Box<dyn Filter> = if let Some(comp) = components.next() {
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

impl Filter for SubdirView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
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

    fn filter_spec(&self) -> String {
        return format!(":/{}", &self.path.to_str().unwrap());
    }
}

struct PrefixView {
    prefix: std::path::PathBuf,
}

impl Filter for PrefixView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        replace_subtree(&repo, &self.prefix, tree.id(), &empty_tree(&repo))
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

    fn filter_spec(&self) -> String {
        return format!(":prefix={}", &self.prefix.to_str().unwrap());
    }
}

struct HideView {
    path: std::path::PathBuf,
}

impl Filter for HideView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> git2::Oid {
        replace_subtree(&repo, &self.path, git2::Oid::zero(), &tree)
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid {
        let hidden = parent_tree
            .get_path(&self.path)
            .map(|x| x.id())
            .unwrap_or(git2::Oid::zero());
        return replace_subtree(&repo, &self.path, hidden, &tree);
    }

    fn filter_spec(&self) -> String {
        return format!(":hide={}", &self.path.to_str().unwrap());
    }
}

struct InfoFileView {
    values: std::collections::BTreeMap<String, String>,
}

impl Filter for InfoFileView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
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

    fn filter_spec(&self) -> String {
        let mut s = format!(":info=");

        for (k, v) in self.values.iter() {
            s = format!("{}{}={},", s, k, v);
        }
        return s.trim_end_matches(",").to_string();
    }
}

struct CombineView {
    base: Box<dyn Filter>,
    others: Vec<Box<dyn Filter>>,
    prefixes: Vec<std::path::PathBuf>,
}

impl Filter for CombineView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        if self.prefixes.len() == 0 {
            return self.base.apply_to_parents(
                &repo,
                &commit,
                forward_maps,
                backward_maps,
            );
        }
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn prefixes(&self) -> HashMap<String, String> {
        let mut p = HashMap::new();
        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            p.insert(prefix.to_str().unwrap().to_owned(), other.filter_spec());
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
            /* let otree = repo.find_tree(otree).expect("can't find tree"); */
            let otree = find_tree_or_error(
                &repo,
                otree,
                repo.find_commit(commit_id).ok().as_ref(),
                &self.filter_spec(),
            );
            base = replace_subtree(
                &repo,
                &prefix,
                otree.id(),
                &repo.find_tree(base).unwrap(),
            );
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

        let mut res = self.base.unapply(
            repo,
            &repo.find_tree(base_wo).unwrap(),
            parent_tree,
        );

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            let r = ok_or!(tree.get_path(&prefix).map(|x| x.id()), {
                continue;
            });
            if r == empty_tree_id() {
                continue;
            }
            let r = repo.find_tree(r).unwrap();
            let ua = other.unapply(&repo, &r, &parent_tree);

            let merged = repo
                .merge_trees(
                    &parent_tree,
                    &repo.find_tree(res).unwrap(),
                    &repo.find_tree(ua).unwrap(),
                    Some(
                        git2::MergeOptions::new()
                            .file_favor(git2::FileFavor::Theirs),
                    ),
                )
                .unwrap()
                .write_tree_to(&repo)
                .unwrap();

            res = merged;
        }

        return res;
    }

    fn filter_spec(&self) -> String {
        let mut s = format!("/ = {}", &self.base.filter_spec());

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            s = format!(
                "{}\n{} = {}",
                &s,
                prefix.to_str().unwrap(),
                other.filter_spec()
            );
        }
        return s;
    }
}

struct WorkspaceView {
    ws_path: std::path::PathBuf,
}

fn combine_view_from_ws(
    repo: &git2::Repository,
    tree: &git2::Tree,
    ws_path: &Path,
) -> Box<CombineView> {
    let base = SubdirView::new(&ws_path);
    let wsp = ws_path.join("workspace.josh");
    let ws_config_oid = ok_or!(tree.get_path(&wsp).map(|x| x.id()), {
        return build_combine_view("", base);
    });

    let ws_blob = ok_or!(repo.find_blob(ws_config_oid), {
        return build_combine_view("", base);
    });

    let ws_content = ok_or!(std::str::from_utf8(ws_blob.content()), {
        return build_combine_view("", base);
    });

    return build_combine_view(ws_content, base);
}

impl WorkspaceView {
    fn ws_apply_to_tree_and_parents(
        &self,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<git2::Oid>),
        commit_id: git2::Oid,
    ) -> super::JoshResult<(git2::Oid, Vec<git2::Oid>)> {
        let (tree, parents) = tree_and_parents;
        let full_tree = repo.find_tree(tree)?;

        let mut in_this = std::collections::HashSet::new();

        let cw = combine_view_from_ws(repo, &full_tree, &self.ws_path);

        for (other, prefix) in cw.others.iter().zip(cw.prefixes.iter()) {
            in_this.insert(format!(
                "{} = {}",
                prefix.to_str().ok_or(super::josh_error("prefix.to_str"))?,
                other.filter_spec()
            ));
        }

        let mut filtered_parent_ids = vec![];
        for parent in parents.iter() {
            let p = apply_view_cached(
                repo,
                self,
                *parent,
                forward_maps,
                backward_maps,
            )?;
            if p != git2::Oid::zero() {
                filtered_parent_ids.push(p);
            }

            let parent_commit = repo.find_commit(*parent)?;

            let pcw = combine_view_from_ws(
                repo,
                &parent_commit.tree()?,
                &self.ws_path,
            );

            for (other, prefix) in pcw.others.iter().zip(pcw.prefixes.iter()) {
                in_this.remove(&format!(
                    "{} = {}",
                    prefix
                        .to_str()
                        .ok_or(super::josh_error("prefix.to_str"))?,
                    other.filter_spec()
                ));
            }
        }
        let mut s = String::new();
        for x in in_this {
            s = format!("{}{}\n", s, x);
        }

        let pcw: Box<dyn Filter> = build_combine_view(&s, Box::new(EmptyView));

        for parent in parents {
            if let Ok(parent) = repo.find_commit(parent) {
                let p = pcw.apply_to_commit(
                    &repo,
                    &parent,
                    forward_maps,
                    backward_maps,
                    &mut HashMap::new(),
                )?;
                if p != git2::Oid::zero() {
                    filtered_parent_ids.push(p);
                }
            }
            break;
        }

        return Ok((
            cw.apply_to_tree(repo, &full_tree, commit_id),
            filtered_parent_ids,
        ));
    }
}

fn find_tree_or_error<'a>(
    repo: &'a git2::Repository,
    filtered_tree: git2::Oid,
    commit: Option<&git2::Commit>,
    filter_spec: &str,
) -> git2::Tree<'a> {
    ok_or!(repo.find_tree(filtered_tree), {
        tracing::debug!(
                    "Filter.apply_to_commit: can't find tree: {:?} filter_spec: {:?}, original-commit: {:?}, message: {:?}, header: {:?}, obj.kind: {:?}",
                    filtered_tree,
                    filter_spec,
                    commit.map(|x| x.id()),
                    commit.map(|x| x.message()),
                    commit.map(|x| x.raw_header()),
                    repo.find_object(filtered_tree, None).ok().map(|x| x.kind()));
        return empty_tree(&repo);
    })
}

impl Filter for WorkspaceView {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_view_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut ViewMaps,
        backward_maps: &mut ViewMaps,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }

        let (filtered_tree, filtered_parent_ids) = self
            .ws_apply_to_tree_and_parents(
                forward_maps,
                backward_maps,
                repo,
                (commit.tree_id(), commit.parents().map(|x| x.id()).collect()),
                commit.id(),
            )?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            &find_tree_or_error(
                &repo,
                filtered_tree,
                Some(&commit),
                &self.filter_spec(),
            ),
        );
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
        let mut cw =
            combine_view_from_ws(repo, tree, &std::path::PathBuf::from(""));

        cw.base = SubdirView::new(&self.ws_path);
        return cw.unapply(repo, tree, parent_tree);
    }

    fn filter_spec(&self) -> String {
        return format!(":workspace={}", &self.ws_path.to_str().unwrap());
    }
}

#[derive(Parser)]
#[grammar = "view_parser.pest"]
struct MyParser;

fn make_view(cmd: &str, name: &str) -> Box<dyn Filter> {
    if cmd == "+" || cmd == "prefix" {
        return Box::new(PrefixView {
            prefix: Path::new(name).to_owned(),
        });
    } else if cmd == "hide" {
        return Box::new(HideView {
            path: Path::new(name).to_owned(),
        });
    } else if cmd == "empty" {
        return Box::new(EmptyView);
    } else if cmd == "info" {
        let mut s = std::collections::BTreeMap::new();
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
            name: name.to_owned(),
        });
    } else if cmd == "workspace" {
        return Box::new(WorkspaceView {
            ws_path: Path::new(name).to_owned(),
        });
    } else {
        return SubdirView::new(&Path::new(name));
    }
}

fn parse_item(pair: pest::iterators::Pair<Rule>) -> Box<dyn Filter> {
    match pair.as_rule() {
        Rule::filter => {
            let mut inner = pair.into_inner();
            make_view(
                inner.next().unwrap().as_str(),
                inner.next().unwrap().as_str(),
            )
        }
        _ => unreachable!(),
    }
}

fn parse_file_entry(
    pair: pest::iterators::Pair<Rule>,
    combine_view: &mut CombineView,
) {
    match pair.as_rule() {
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let view = inner.next().unwrap().as_str();
            let view = parse(view);
            combine_view.prefixes.push(Path::new(path).to_owned());
            combine_view.others.push(view);
        }
        _ => unreachable!(),
    }
}

fn build_combine_view(
    filter_spec: &str,
    base: Box<dyn Filter>,
) -> Box<CombineView> {
    let mut combine_view = Box::new(CombineView {
        base: base,
        others: vec![],
        prefixes: vec![],
    });

    if let Ok(r) = MyParser::parse(Rule::workspace_file, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            parse_file_entry(pair, &mut combine_view);
        }
    };

    return combine_view;
}

pub fn build_chain(
    first: Box<dyn Filter>,
    second: Box<dyn Filter>,
) -> Box<dyn Filter> {
    Box::new(ChainView {
        first: first,
        second: second,
    })
}

pub fn parse(filter_spec: &str) -> Box<dyn Filter> {
    if filter_spec == "" {
        return parse(":nop=nop");
    }
    if filter_spec.starts_with("!") || filter_spec.starts_with(":") {
        let mut chain: Option<Box<dyn Filter>> = None;
        if let Ok(r) = MyParser::parse(Rule::filter_spec, filter_spec) {
            let mut r = r;
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                let v = parse_item(pair);
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

    return build_combine_view(filter_spec, Box::new(EmptyView));
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
        if oid == git2::Oid::zero() {
            builder.remove(child).ok();
        } else {
            builder
                .insert(child, oid, mode)
                .expect("replace_child: can't insert tree");
        }
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
            empty_tree(&repo)
        };

        let tree = repo
            .find_tree(replace_child(&repo, name, oid, &st))
            .expect("replace_child: can't find new tree");

        return replace_subtree(&repo, path, tree.id(), full_tree);
    }
}

fn apply_view_cached(
    repo: &git2::Repository,
    view: &dyn Filter,
    newrev: git2::Oid,
    forward_maps: &mut view_maps::ViewMaps,
    backward_maps: &mut view_maps::ViewMaps,
) -> super::JoshResult<git2::Oid> {
    if forward_maps.has(repo, &view.filter_spec(), newrev) {
        return Ok(forward_maps.get(&view.filter_spec(), newrev));
    }

    let trace_s = tracing::span!(tracing::Level::TRACE, "apply_view_cached", filter_spec = ?view.filter_spec());

    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(newrev)?;
        walk
    };

    let mut in_commit_count = 0;
    let mut out_commit_count = 0;
    let mut empty_tree_count = 0;
    for original_commit_id in walk {
        in_commit_count += 1;

        let original_commit = repo.find_commit(original_commit_id?)?;

        let filtered_commit = ok_or!(
            view.apply_to_commit(
                &repo,
                &original_commit,
                forward_maps,
                backward_maps,
                &mut HashMap::new(),
            ),
            {
                tracing::error!("cannot apply_to_commit");
                git2::Oid::zero()
            }
        );

        if filtered_commit == git2::Oid::zero() {
            empty_tree_count += 1;
        }
        forward_maps.set(
            &view.filter_spec(),
            original_commit.id(),
            filtered_commit,
        );
        backward_maps.set(
            &view.filter_spec(),
            filtered_commit,
            original_commit.id(),
        );
        out_commit_count += 1;
    }

    if !forward_maps.has(&repo, &view.filter_spec(), newrev) {
        forward_maps.set(&view.filter_spec(), newrev, git2::Oid::zero());
    }
    let rewritten = forward_maps.get(&view.filter_spec(), newrev);
    tracing::event!(
        parent: &trace_s,
        tracing::Level::TRACE,
        ?in_commit_count,
        ?out_commit_count,
        ?empty_tree_count,
        original = ?newrev.to_string(),
        rewritten = ?rewritten.to_string(),
    );
    return Ok(rewritten);
}
