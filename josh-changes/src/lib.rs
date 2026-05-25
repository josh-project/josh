use anyhow::anyhow;

#[derive(Debug, Clone)]
pub struct Change {
    author: String,
    id: Option<String>,
    series: Vec<String>,
    commit: git2::Oid,
    base: git2::Oid,
}

impl Change {
    pub fn new(repo: &git2::Repository, commit: &git2::Commit) -> Self {
        let mut change = Self {
            author: commit.author().email().unwrap_or("").to_string(),
            id: None,
            series: Vec::new(),
            commit: commit.id(),
            base: git2::Oid::zero(),
        };
        let (id, series) = commit_change_meta(commit);
        change.id = id;
        change.series = series;

        if change.id().is_some() {
            let _ = store_diff_data(repo, &change);
        }

        change
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn series(&self) -> &[String] {
        &self.series
    }

    pub fn commit(&self) -> git2::Oid {
        self.commit
    }

    pub fn base(&self) -> git2::Oid {
        self.base
    }

    pub fn contributing(&self, repo: &git2::Repository) -> anyhow::Result<Vec<git2::Oid>> {
        let mut walk = repo.revwalk()?;
        walk.simplify_first_parent()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        walk.push(self.commit)?;
        if self.base != git2::Oid::zero() {
            walk.hide(self.base)?;
        }
        let mut oids: Vec<git2::Oid> = walk.collect::<Result<Vec<_>, _>>()?;
        if oids.first() == Some(&self.commit) {
            oids.remove(0);
        }
        Ok(oids)
    }
}

fn is_trailer_line(line: &str) -> bool {
    let key_len = line
        .bytes()
        .take_while(|&b| b.is_ascii_alphanumeric() || b == b'-')
        .count();
    key_len > 0 && line[key_len..].starts_with(": ")
}

/// Extract change-id metadata from a commit, preferring jj/gitbutler's custom
/// `change-id` commit-object header over any `Change:` / `Change-Id:` trailer
/// in the message body. The series list comes from message trailers regardless.
pub fn commit_change_meta(commit: &git2::Commit) -> (Option<String>, Vec<String>) {
    let (mut id, series) = parse_change_meta(commit.message().unwrap_or(""));
    if let Ok(buf) = commit.header_field_bytes("change-id") {
        if let Ok(s) = std::str::from_utf8(&buf) {
            let s = s.trim();
            if !s.is_empty() {
                id = Some(s.to_string());
            }
        }
    }
    (id, series)
}

pub fn parse_change_meta(message: &str) -> (Option<String>, Vec<String>) {
    let lines: Vec<&str> = message.lines().collect();
    let mut footer_start = lines.len();
    for (i, line) in lines.iter().enumerate().rev() {
        if line.is_empty() || is_trailer_line(line) {
            footer_start = i;
        } else {
            break;
        }
    }

    let mut id: Option<String> = None;
    let mut series: Vec<String> = Vec::new();
    for line in &lines[footer_start..] {
        if let Some(v) = line.strip_prefix("Change: ") {
            id = Some(v.to_string());
        }
        if let Some(v) = line.strip_prefix("Change-Id: ") {
            id = Some(v.to_string());
        }
        if let Some(v) = line.strip_prefix("Change-Series: ") {
            series.push(v.to_string());
        }
    }
    (id, series)
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
) -> anyhow::Result<Vec<Change>> {
    if changes.values().next().map(|c| c.base) == Some(git2::Oid::zero()) {
        return Ok(changes.into_values().collect());
    }

    changes.iter().map(|(_, c)| downstack(repo, c)).collect()
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

fn downstack(repo: &git2::Repository, change: &Change) -> anyhow::Result<Change> {
    let change_oid = change.commit;
    if !repo.graph_descendant_of(change_oid, change.base)? {
        return Err(anyhow!(
            "change {} is not a descendant of base {}",
            change_oid,
            change.base
        ));
    }

    // Collect commits from base to change (exclusive of base, inclusive of change)
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
    walk.push(change_oid)?;
    walk.hide(change.base)?;

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
    let change_meta = Change::new(repo, &change_commit);
    let mut affected_paths = changed_paths(repo, &change_commit)?;
    for s in &change_meta.series {
        affected_paths.insert(format!("\x00series:{}", s));
    }
    let mut needed: Vec<bool> = vec![false; commits.len()];
    for (i, intermediate) in commits.iter().enumerate().rev() {
        let meta = Change::new(repo, intermediate);
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
    let mut current_base = repo.find_commit(change.base)?;
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
        let mut change = Change::new(repo, &commit);
        if change.id.is_none() {
            continue;
        }
        change.base = base;
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
            let changes = split_changes(repo, changes)?;

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

pub fn list_changes(
    repo: &git2::Repository,
    tip: git2::Oid,
    base: git2::Oid,
) -> anyhow::Result<Vec<Change>> {
    let changes = get_changes(repo, tip, base)?;
    split_changes(repo, changes)
}

pub fn resolve_change(
    repo: &git2::Repository,
    head: git2::Oid,
    spec: &str,
) -> anyhow::Result<Change> {
    // Try as a full OID first.
    if let Ok(oid) = git2::Oid::from_str(spec) {
        if let Ok(commit) = repo.find_commit(oid) {
            return Ok(Change::new(repo, &commit));
        }
    }

    // Try as a revparse (branch, tag, short SHA).
    if let Ok(obj) = repo.revparse_single(spec) {
        if let Ok(commit) = obj.peel_to_commit() {
            return Ok(Change::new(repo, &commit));
        }
    }

    // Walk from head to find a commit with matching Change-Id.
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(head)?;
    for oid in walk {
        let oid = oid?;
        if let Ok(c) = repo.find_commit(oid) {
            let (id, _) = parse_change_meta(c.message().unwrap_or(""));
            if id.as_deref() == Some(spec) {
                return Ok(Change::new(repo, &c));
            }
        }
    }

    Err(anyhow!("could not resolve '{}' to a commit", spec))
}

pub fn diff_id(repo: &git2::Repository, commit_oid: git2::Oid) -> anyhow::Result<String> {
    let commit = repo.find_commit(commit_oid)?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;

    let mut buf = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        buf.extend_from_slice(&[line.origin() as u8]);
        buf.extend_from_slice(line.content());
        true
    })?;

    Ok(git2::Oid::hash_object(git2::ObjectType::Blob, &buf)?.to_string())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CommentMeta {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<Location>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub update_of: Option<String>,
}

pub fn write_comment(
    repo: &git2::Repository,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
) -> anyhow::Result<String> {
    write_comment_with_diff(repo, change, meta, author, timestamp, None)
}

pub fn write_comment_with_diff(
    repo: &git2::Repository,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    diff_id_override: Option<&str>,
) -> anyhow::Result<String> {
    if meta.message.trim().is_empty() {
        return Err(anyhow::anyhow!("comment message must not be empty"));
    }

    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;
    let diff_id = match diff_id_override {
        Some(d) => d.to_string(),
        None => diff_id(repo, change.commit())?,
    };

    let content = serde_json::to_string(meta)?;
    let content_hash =
        git2::Oid::hash_object(git2::ObjectType::Blob, content.as_bytes())?.to_string();
    let blob_oid = repo.blob(content.as_bytes())?;

    let path = std::path::Path::new("comments")
        .join(&change_id)
        .join(&diff_id)
        .join(&content_hash);
    write_changes_tree(repo, &path, blob_oid, author, timestamp)?;

    Ok(content_hash)
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub id: String,
    pub message: String,
    pub file: Option<String>,
    pub location: Option<Location>,
    pub reply_to: Option<String>,
    pub update_of: Option<String>,
}

pub fn read_comments(repo: &git2::Repository, change: &Change) -> anyhow::Result<Vec<Comment>> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };
    let diff_id = diff_id(repo, change.commit())?;

    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Vec::new()),
    };

    let comments_tree = match tree.get_name("comments") {
        Some(e) => e.to_object(repo)?.peel_to_tree()?,
        None => return Ok(Vec::new()),
    };
    let cid_tree = match comments_tree.get_name(change_id) {
        Some(e) => e.to_object(repo)?.peel_to_tree()?,
        None => return Ok(Vec::new()),
    };
    let did_tree = match cid_tree.get_name(&diff_id) {
        Some(e) => e.to_object(repo)?.peel_to_tree()?,
        None => return Ok(Vec::new()),
    };

    let mut comments = Vec::new();
    for entry in did_tree.iter() {
        let id = entry.name().unwrap_or("").to_string();
        let blob = entry.to_object(repo)?.peel_to_blob()?;
        let meta: CommentMeta = serde_json::from_slice(blob.content())?;
        comments.push(Comment {
            id,
            message: meta.message,
            file: meta.file,
            location: meta.location,
            reply_to: meta.reply_to,
            update_of: meta.update_of,
        });
    }
    Ok(comments)
}

pub fn store_diff_data(repo: &git2::Repository, change: &Change) -> anyhow::Result<()> {
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;
    let diff_id = diff_id(repo, change.commit())?;

    let commit_oid_str = change.commit().to_string();
    let blob_oid = repo.blob(commit_oid_str.as_bytes())?;

    let path = std::path::Path::new("diffs")
        .join(&change_id)
        .join(&diff_id)
        .join(&commit_oid_str);
    write_changes_tree(repo, &path, blob_oid, None, None)?;

    Ok(())
}

fn parse_timestamp(s: Option<&str>) -> git2::Time {
    let Some(s) = s else {
        return git2::Time::new(0, 0);
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) else {
        return git2::Time::new(0, 0);
    };
    git2::Time::new(dt.timestamp(), dt.offset().local_minus_utc() / 60)
}

fn write_changes_tree(
    repo: &git2::Repository,
    path: &std::path::Path,
    blob_oid: git2::Oid,
    author: Option<&str>,
    timestamp: Option<&str>,
) -> anyhow::Result<()> {
    let base_tree = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    // Skip if the blob already exists at this path.
    if let Some(existing) = base_tree
        .get_path(path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == blob_oid {
            return Ok(());
        }
    }

    let tree = josh_core::filter::tree::insert(
        repo,
        &base_tree,
        path,
        blob_oid,
        git2::FileMode::Blob.into(),
    )?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => josh_core::git::user_signature(repo)?,
    };
    let parent_commit = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    repo.commit(
        Some("refs/josh/changes"),
        &sig,
        &sig,
        "update refs/josh/changes\n",
        &tree,
        &parents,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_in_body_is_ignored() {
        let (id, series) =
            parse_change_meta("Subject\n\nbody mentions Change: not-a-trailer\nmore body\n");
        assert_eq!(id, None);
        assert!(series.is_empty());
    }

    #[test]
    fn real_trailing_footer_is_parsed() {
        let (id, _) = parse_change_meta("Subject\n\nBody.\n\nChange: real-id\n");
        assert_eq!(id.as_deref(), Some("real-id"));
    }

    #[test]
    fn single_line_message_is_its_own_footer() {
        let (id, _) = parse_change_meta("Change: only-line");
        assert_eq!(id.as_deref(), Some("only-line"));
    }

    #[test]
    fn footer_followed_by_body_is_ignored() {
        let (id, _) = parse_change_meta("Subject\n\nChange: middle\n\nBody after.\n");
        assert_eq!(id, None);
    }

    #[test]
    fn other_trailers_in_block_do_not_break_change() {
        let msg = "Subject\n\nBody.\n\nSigned-off-by: x <x@y>\nChange: real\n\
                   Reviewed-by: z <z@w>\n";
        let (id, _) = parse_change_meta(msg);
        assert_eq!(id.as_deref(), Some("real"));
    }

    #[test]
    fn series_in_footer_block_is_collected() {
        let msg = "Subject\n\nBody.\n\nChange-Series: s1\nChange-Series: s2\nChange: c\n";
        let (id, series) = parse_change_meta(msg);
        assert_eq!(id.as_deref(), Some("c"));
        assert_eq!(series, vec!["s1".to_string(), "s2".to_string()]);
    }

    #[test]
    fn series_in_body_is_ignored() {
        let msg = "Subject\n\nWe discussed Change-Series: bogus here.\nmore body\n";
        let (_id, series) = parse_change_meta(msg);
        assert!(series.is_empty());
    }

    #[test]
    fn is_trailer_line_basics() {
        assert!(is_trailer_line("Change: foo"));
        assert!(is_trailer_line("Change-Id: foo"));
        assert!(is_trailer_line("Signed-off-by: a <a@b>"));
        assert!(!is_trailer_line("not a trailer"));
        assert!(!is_trailer_line("Change:no-space"));
        assert!(!is_trailer_line(": leading colon"));
        assert!(!is_trailer_line(""));
    }
}
