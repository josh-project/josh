use anyhow::anyhow;

#[derive(Debug, Clone)]
struct Change {
    pub author: String,
    pub id: Option<String>,
    pub requires: Vec<String>,
    pub commit: git2::Oid,
}

impl Change {
    fn new(commit: git2::Oid) -> Self {
        Self {
            author: Default::default(),
            id: Default::default(),
            requires: Default::default(),
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
        if let Some(id) = line.strip_prefix("Requires: ") {
            change.requires.push(id.to_string());
        }
    }
    change
}

#[derive(PartialEq, Clone, Debug)]
pub enum PushMode {
    Normal,
    Stack(String),
    Split(String),
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
    if baseref.starts_with("refs/stack/for") {
        push_mode = PushMode::Stack(author.to_string());
        baseref = baseref.replacen("refs/stack/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/split/for") {
        push_mode = PushMode::Split(author.to_string());
        baseref = baseref.replacen("refs/split/for", "refs/heads", 1)
    }
    Ok((baseref, push_to, options, push_mode))
}

fn add_base_refs(repo: &git2::Repository, refs: &mut Vec<PushRef>) -> anyhow::Result<()> {
    let original_refs = std::mem::take(refs);
    for push_ref in original_refs.into_iter() {
        let base_ref = push_ref
            .ref_name
            .replacen("refs/heads/@changes", "refs/heads/@base", 1);

        let oid = push_ref.oid;
        let change_id = push_ref.change_id.clone();
        refs.push(push_ref);

        if let Some(parent_sha) = repo.find_commit(oid)?.parent_ids().next() {
            refs.push(PushRef {
                ref_name: base_ref,
                oid: parent_sha,
                change_id,
            });
        }
    }

    Ok(())
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
        .map(|(_, c)| downstack(repo, base, c, &changes))
        .collect()
}

fn downstack(
    repo: &git2::Repository,
    base: git2::Oid,
    change: &Change,
    all_changes: &std::collections::HashMap<git2::Oid, Change>,
) -> anyhow::Result<Change> {
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

    // Use pre-parsed requires, keeping only those referencing changes
    // actually present in the intermediates
    let intermediate_ids: Vec<Option<&str>> = commits
        .iter()
        .map(|c| all_changes.get(&c.id()).and_then(|ch| ch.id.as_deref()))
        .collect();
    let available_ids: std::collections::HashSet<&str> =
        intermediate_ids.iter().filter_map(|id| *id).collect();
    let mut required: std::collections::HashSet<&str> = change
        .requires
        .iter()
        .map(|s| s.as_str())
        .filter(|id| available_ids.contains(id))
        .collect();

    // Walk through intermediates, including only those needed for
    // the change to apply cleanly.
    let mut current_base = repo.find_commit(base)?;

    for (intermediate, change_id) in commits.iter().zip(intermediate_ids.iter()) {
        // Stop when the change merges cleanly and all Requires: are satisfied
        let diff_applies = repo
            .merge_trees(
                &change_parent.tree()?,
                &current_base.tree()?,
                &change_commit.tree()?,
                None,
            )
            .map(|index| !index.has_conflicts())
            .unwrap_or(false);
        if diff_applies && required.is_empty() {
            break;
        }

        // Change does not apply yet; we need this intermediate commit.
        // Rebase it onto current_base via 3-way merge.
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

        if let Some(id) = change_id {
            required.remove(id);
        }
    }

    // Apply the change on top of the minimal base via 3-way merge
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

    Ok(changes
        .into_iter()
        .filter_map(|change| {
            change.id.map(|change_id| PushRef {
                ref_name: format!(
                    "refs/heads/@changes/{}/{}/{}",
                    baseref.replacen("refs/heads/", "", 1),
                    change.author,
                    change_id,
                ),
                oid: change.commit,
                change_id,
            })
        })
        .collect())
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
        PushMode::Stack(author) | PushMode::Split(author) => {
            let changes = get_changes(repo, oid_to_push, base_oid)?;

            let changes = if matches!(push_mode, PushMode::Split(_)) {
                split_changes(repo, changes, base_oid)?
            } else {
                changes.into_values().collect()
            };

            let mut push_refs = changes_to_refs(baseref, author, changes)?;

            add_base_refs(repo, &mut push_refs)?;

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
