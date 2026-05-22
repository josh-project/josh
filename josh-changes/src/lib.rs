use anyhow::anyhow;

#[derive(Debug, Clone)]
struct Change {
    pub author: String,
    pub id: Option<String>,
    pub series: Vec<String>,
    pub commit: git2::Oid,
}

impl Change {
    fn new(commit: git2::Oid) -> Self {
        Self {
            author: Default::default(),
            id: Default::default(),
            series: Default::default(),
            commit,
        }
    }
}

fn get_change_id(commit: &git2::Commit) -> Change {
    let mut change = Change::new(commit.id());
    change.author = commit.author().email().unwrap_or("").to_string();

    for line in commit.message().unwrap_or("").lines() {
        if let Some(id) = line.strip_prefix("Change: ") {
            change.id = Some(id.to_string());
        }
        if let Some(id) = line.strip_prefix("Change-Id: ") {
            change.id = Some(id.to_string());
        }
        if let Some(s) = line.strip_prefix("Change-Series: ") {
            change.series.push(s.to_string());
        }
    }
    change
}

#[derive(PartialEq, Clone, Debug)]
pub enum PushMode {
    Normal,
    Publish(String),
}

#[derive(Debug, Clone)]
pub struct PushRef {
    pub ref_name: String,
    pub oid: git2::Oid,
    pub change_id: String,
}

pub fn baseref_and_options(
    refname: &str,
    author: &str,
) -> anyhow::Result<(String, String, Vec<String>, PushMode)> {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().ok_or(anyhow!("no next"))?.to_owned();

    let options = if let Some(options) = split.next() {
        options.split(',').map(|x| x.to_string()).collect()
    } else {
        vec![]
    };

    let mut baseref = push_to.to_owned();
    let mut push_mode = PushMode::Normal;

    if baseref.starts_with("refs/for") {
        baseref = baseref.replacen("refs/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/drafts") {
        baseref = baseref.replacen("refs/drafts", "refs/heads", 1)
    }
    if baseref.starts_with("refs/publish/for") {
        push_mode = PushMode::Publish(author.to_string());
        baseref = baseref.replacen("refs/publish/for", "refs/heads", 1)
    }
    Ok((baseref, push_to, options, push_mode))
}

fn split_changes(
    repo: &git2::Repository,
    changes: std::collections::HashMap<git2::Oid, Change>,
    base: git2::Oid,
) -> anyhow::Result<Vec<Change>> {
    if base == git2::Oid::zero() {
        return Ok(changes.into_values().collect());
    }

    changes
        .iter()
        .map(|(_, c)| downstack(repo, base, c))
        .collect()
}

fn changed_paths(
    repo: &git2::Repository,
    commit: &git2::Commit,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;
    let mut paths = std::collections::HashSet::new();
    for delta in diff.deltas() {
        if let Some(p) = delta.old_file().path().and_then(|p| p.to_str()) {
            paths.insert(p.to_string());
        }
        if let Some(p) = delta.new_file().path().and_then(|p| p.to_str()) {
            paths.insert(p.to_string());
        }
    }
    Ok(paths)
}

fn downstack(repo: &git2::Repository, base: git2::Oid, change: &Change) -> anyhow::Result<Change> {
    let change_oid = change.commit;
    if !repo.graph_descendant_of(change_oid, base)? {
        return Err(anyhow!(
            "change {} is not a descendant of base {}",
            change_oid,
            base
        ));
    }

    // Collect commits from base to change (exclusive of base, inclusive of change)
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
    walk.push(change_oid)?;
    walk.hide(base)?;

    let oids: Vec<git2::Oid> = walk.collect::<Result<Vec<_>, _>>()?;

    if oids.is_empty() {
        return Ok(change.clone());
    }

    let mut commits: Vec<git2::Commit> = oids
        .into_iter()
        .map(|oid| repo.find_commit(oid))
        .collect::<Result<Vec<_>, _>>()?;

    // The last commit is `change`; split it off from the intermediates
    let change_commit = commits.pop().unwrap();
    let change_parent = change_commit.parent(0)?;

    // Seed the affected path set with the change's own modified paths, then
    // walk intermediates backwards, keeping any commit whose paths intersect.
    let change_meta = get_change_id(&change_commit);
    let mut affected_paths = changed_paths(repo, &change_commit)?;
    for s in &change_meta.series {
        affected_paths.insert(format!("\x00series:{}", s));
    }
    let mut needed: Vec<bool> = vec![false; commits.len()];
    for (i, intermediate) in commits.iter().enumerate().rev() {
        let meta = get_change_id(intermediate);
        let mut paths = changed_paths(repo, intermediate)?;
        for s in &meta.series {
            paths.insert(format!("\x00series:{}", s));
        }
        if !paths.is_disjoint(&affected_paths) {
            needed[i] = true;
            affected_paths.extend(paths);
        }
    }

    // Rebase needed intermediates forward onto current_base.
    let mut current_base = repo.find_commit(base)?;
    for (intermediate, is_needed) in commits.iter().zip(needed.iter()) {
        if !is_needed {
            continue;
        }
        let inter_parent = intermediate.parent(0)?;
        let mut index = repo.merge_trees(
            &inter_parent.tree()?,
            &current_base.tree()?,
            &intermediate.tree()?,
            None,
        )?;
        let new_tree = repo.find_tree(index.write_tree_to(repo)?)?;
        let new_oid = josh_core::history::rewrite_commit(
            repo,
            intermediate,
            &[&current_base],
            josh_core::filter::Rewrite::from_tree(new_tree),
            josh_core::history::GpgsigMode::Preserve,
        )?;
        current_base = repo.find_commit(new_oid)?;
    }

    // Apply the change on top of the minimal base via 3-way merge.
    let mut index = repo.merge_trees(
        &change_parent.tree()?,
        &current_base.tree()?,
        &change_commit.tree()?,
        None,
    )?;
    let new_tree = repo.find_tree(index.write_tree_to(repo)?)?;

    let new_oid = josh_core::history::rewrite_commit(
        repo,
        &change_commit,
        &[&current_base],
        josh_core::filter::Rewrite::from_tree(new_tree),
        josh_core::history::GpgsigMode::Preserve,
    )?;

    let mut result = change.clone();
    result.commit = new_oid;
    Ok(result)
}

fn changes_to_refs(
    repo: &git2::Repository,
    baseref: &str,
    change_author: &str,
    changes: Vec<Change>,
) -> anyhow::Result<Vec<PushRef>> {
    if !change_author.contains('@') {
        return Err(anyhow!(
            "Push option 'author' needs to be set to a valid email address",
        ));
    };

    let changes: Vec<Change> = changes
        .into_iter()
        .filter(|change| change.author == change_author)
        .collect();

    let mut seen = std::collections::HashSet::new();
    for change in changes.iter() {
        if let Some(id) = &change.id {
            if id.contains('@') {
                return Err(anyhow!("Change id must not contain '@'"));
            }
            if !seen.insert(id) {
                return Err(anyhow!(
                    "rejecting to push {:?} with duplicate label",
                    change.commit
                ));
            }
            seen.insert(id);
        }
    }

    let mut refs = vec![];
    for change in changes {
        if let Some(change_id) = change.id {
            let ref_name = format!(
                "refs/heads/@changes/{}/{}/{}",
                baseref.replacen("refs/heads/", "", 1),
                change.author,
                change_id,
            );
            let base_ref_name = ref_name.replacen("refs/heads/@changes", "refs/heads/@base", 1);
            refs.push(PushRef {
                ref_name,
                oid: change.commit,
                change_id: change_id.clone(),
            });
            if let Some(parent_sha) = repo.find_commit(change.commit)?.parent_ids().next() {
                refs.push(PushRef {
                    ref_name: base_ref_name,
                    oid: parent_sha,
                    change_id,
                });
            }
        }
    }
    Ok(refs)
}

fn get_changes(
    repo: &git2::Repository,
    tip: git2::Oid,
    base: git2::Oid,
) -> anyhow::Result<std::collections::HashMap<git2::Oid, Change>> {
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
    walk.simplify_first_parent()?;
    walk.push(tip)?;
    if base != git2::Oid::zero() {
        walk.hide(base)?;
    }

    let mut changes = std::collections::HashMap::new();
    for rev in walk {
        let commit = repo.find_commit(rev?)?;
        let change = get_change_id(&commit);
        changes.insert(change.commit, change);
    }

    Ok(changes)
}

pub fn build_to_push(
    repo: &git2::Repository,
    push_mode: &PushMode,
    baseref: &str,
    ref_with_options: &str,
    oid_to_push: git2::Oid,
    base_oid: git2::Oid,
) -> anyhow::Result<Vec<PushRef>> {
    match push_mode {
        PushMode::Publish(author) => {
            let changes = get_changes(repo, oid_to_push, base_oid)?;
            let changes = split_changes(repo, changes, base_oid)?;

            let mut push_refs = changes_to_refs(repo, baseref, author, changes)?;

            push_refs.push(PushRef {
                ref_name: format!(
                    "refs/heads/@heads/{}/{}",
                    baseref.replacen("refs/heads/", "", 1),
                    author,
                ),
                oid: oid_to_push,
                change_id: baseref.replacen("refs/heads/", "", 1),
            });

            push_refs.sort_by(|a, b| a.ref_name.cmp(&b.ref_name));
            Ok(push_refs)
        }
        PushMode::Normal => Ok(vec![PushRef {
            ref_name: if ref_with_options.starts_with("refs/") {
                ref_with_options.to_string()
            } else {
                format!("refs/heads/{}", ref_with_options)
            },
            oid: oid_to_push,
            change_id: "JOSH_PUSH".to_string(),
        }]),
    }
}
