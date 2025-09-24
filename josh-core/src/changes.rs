use super::*;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum PushMode {
    Normal,
    Review,
    Stack,
    Split,
}

pub fn baseref_and_options(refname: &str) -> JoshResult<(String, String, Vec<String>, PushMode)> {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().ok_or(josh_error("no next"))?.to_owned();

    let options = if let Some(options) = split.next() {
        options.split(',').map(|x| x.to_string()).collect()
    } else {
        vec![]
    };

    let mut baseref = push_to.to_owned();
    let mut push_mode = PushMode::Normal;

    if baseref.starts_with("refs/for") {
        push_mode = PushMode::Review;
        baseref = baseref.replacen("refs/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/drafts") {
        push_mode = PushMode::Review;
        baseref = baseref.replacen("refs/drafts", "refs/heads", 1)
    }
    if baseref.starts_with("refs/stack/for") {
        push_mode = PushMode::Stack;
        baseref = baseref.replacen("refs/stack/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/split/for") {
        push_mode = PushMode::Split;
        baseref = baseref.replacen("refs/split/for", "refs/heads", 1)
    }
    Ok((baseref, push_to, options, push_mode))
}

pub fn split_changes(
    repo: &git2::Repository,
    changes: &mut Vec<(String, git2::Oid, String)>,
    base: git2::Oid,
) -> JoshResult<()> {
    if base == git2::Oid::zero() {
        return Ok(());
    }

    let commits: Vec<git2::Commit> = changes
        .iter()
        .map(|(_, commit, _)| repo.find_commit(*commit).unwrap())
        .collect();

    let mut trees: Vec<git2::Tree> = commits
        .iter()
        .map(|commit| commit.tree().unwrap())
        .collect();

    trees.insert(0, repo.find_commit(base)?.tree()?);

    let diffs: Vec<git2::Diff> = (1..trees.len())
        .map(|i| {
            repo.diff_tree_to_tree(Some(&trees[i - 1]), Some(&trees[i]), None)
                .unwrap()
        })
        .collect();

    let mut moved = std::collections::HashSet::new();
    let mut bases = vec![base];
    for _ in 0..changes.len() {
        let mut new_bases = vec![];
        for base in bases.iter() {
            for i in 0..diffs.len() {
                if moved.contains(&i) {
                    continue;
                }
                let diff = &diffs[i];
                let parent = repo.find_commit(*base)?;
                if let Ok(mut index) = repo.apply_to_tree(&parent.tree()?, diff, None) {
                    moved.insert(i);
                    let new_tree = repo.find_tree(index.write_tree_to(repo)?)?;
                    let new_commit = history::rewrite_commit(
                        repo,
                        &repo.find_commit(changes[i].1)?,
                        &[&parent],
                        history::RewriteData {
                            tree: new_tree,
                            author: None,
                            committer: None,
                            message: None,
                        },
                        false,
                    )?;
                    changes[i].1 = new_commit;
                    new_bases.push(new_commit);
                }
                if moved.len() == changes.len() {
                    return Ok(());
                }
            }
        }
        bases = new_bases;
    }

    Ok(())
}

pub fn changes_to_refs(
    baseref: &str,
    change_author: &str,
    changes: Vec<Change>,
) -> JoshResult<Vec<(String, git2::Oid, String)>> {
    let mut seen = vec![];
    let mut changes = changes;
    changes.retain(|change| change.author == change_author);
    if !change_author.contains('@') {
        return Err(josh_error(
            "Push option 'author' needs to be set to a valid email address",
        ));
    };

    for change in changes.iter() {
        if let Some(id) = &change.id {
            if id.contains('@') {
                return Err(josh_error("Change id must not contain '@'"));
            }
            if seen.contains(&id) {
                return Err(josh_error(&format!(
                    "rejecting to push {:?} with duplicate label",
                    change.commit
                )));
            }
            seen.push(id);
        } else {
            return Err(josh_error(&format!(
                "rejecting to push {:?} without id",
                change.commit
            )));
        }
    }

    Ok(changes
        .iter()
        .map(|change| {
            (
                format!(
                    "refs/heads/@changes/{}/{}/{}",
                    baseref.replacen("refs/heads/", "", 1),
                    change.author,
                    change.id.as_ref().unwrap_or(&"".to_string()),
                ),
                change.commit,
                change
                    .id
                    .as_ref()
                    .unwrap_or(&"JOSH_PUSH".to_string())
                    .to_string(),
            )
        })
        .collect())
}

pub fn build_to_push(
    repo: &git2::Repository,
    changes: Option<Vec<Change>>,
    push_mode: PushMode,
    baseref: &str,
    author: &str,
    ref_with_options: String,
    oid_to_push: git2::Oid,
    old: git2::Oid,
) -> JoshResult<Vec<(String, git2::Oid, String)>> {
    if let Some(changes) = changes {
        let mut v = vec![];
        let mut refs = changes_to_refs(baseref, author, changes)?;
        v.append(&mut refs);
        if push_mode == PushMode::Split {
            split_changes(repo, &mut v, old)?;
        }
        if push_mode == PushMode::Review {
            v.push((
                ref_with_options.clone(),
                oid_to_push,
                "JOSH_PUSH".to_string(),
            ));
        }
        v.push((
            format!(
                "refs/heads/@heads/{}/{}",
                baseref.replacen("refs/heads/", "", 1),
                author,
            ),
            oid_to_push,
            baseref.replacen("refs/heads/", "", 1),
        ));
        Ok(v)
    } else {
        Ok(vec![(
            ref_with_options,
            oid_to_push,
            "JOSH_PUSH".to_string(),
        )])
    }
}
