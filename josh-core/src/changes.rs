use super::*;
use anyhow::anyhow;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum PushMode {
    Normal,
    Review,
    Stack,
    Split,
}

#[derive(Debug, Clone)]
pub struct PushRef {
    pub ref_name: String,
    pub oid: git2::Oid,
    pub change_id: String,
}

pub fn baseref_and_options(
    refname: &str,
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
    changes: &mut [PushRef],
    base: git2::Oid,
) -> anyhow::Result<()> {
    if base == git2::Oid::zero() {
        return Ok(());
    }

    let commits: Vec<git2::Commit> = changes
        .iter()
        .map(|push_ref| repo.find_commit(push_ref.oid))
        .collect::<Result<Vec<_>, _>>()?;

    let mut trees = vec![repo.find_commit(base)?.tree()?];
    trees.extend(
        commits
            .iter()
            .map(|commit| commit.tree())
            .collect::<Result<Vec<_>, _>>()?,
    );

    let diffs: Vec<git2::Diff> = trees
        .windows(2)
        .map(|window| repo.diff_tree_to_tree(Some(&window[0]), Some(&window[1]), None))
        .collect::<Result<Vec<_>, _>>()?;

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
                        &repo.find_commit(changes[i].oid)?,
                        &[&parent],
                        filter::Rewrite::from_tree(new_tree),
                        false,
                    )?;
                    changes[i].oid = new_commit;
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

#[allow(clippy::too_many_arguments)]
pub fn build_to_push(
    repo: &git2::Repository,
    changes: Option<Vec<Change>>,
    push_mode: PushMode,
    baseref: &str,
    author: &str,
    ref_with_options: &str,
    oid_to_push: git2::Oid,
    base_oid: git2::Oid,
) -> anyhow::Result<Vec<PushRef>> {
    if let Some(changes) = changes {
        let mut push_refs = changes_to_refs(baseref, author, changes)?;

        if push_mode == PushMode::Split {
            split_changes(repo, &mut push_refs, base_oid)?;
        }

        add_base_refs(repo, &mut push_refs)?;

        if push_mode == PushMode::Review {
            push_refs.push(PushRef {
                ref_name: ref_with_options.to_string(),
                oid: oid_to_push,
                change_id: "JOSH_PUSH".into(),
            });
        }

        push_refs.push(PushRef {
            ref_name: format!(
                "refs/heads/@heads/{}/{}",
                baseref.replacen("refs/heads/", "", 1),
                author,
            ),
            oid: oid_to_push,
            change_id: baseref.replacen("refs/heads/", "", 1),
        });

        Ok(push_refs)
    } else {
        Ok(vec![PushRef {
            ref_name: ref_with_options.to_string(),
            oid: oid_to_push,
            change_id: "JOSH_PUSH".to_string(),
        }])
    }
}
