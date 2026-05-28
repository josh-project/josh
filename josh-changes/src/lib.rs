use anyhow::anyhow;
pub use josh_core::trailers::{commit_change_meta, parse_change_meta};

#[derive(Debug, Clone)]
pub struct Change {
    author: String,
    id: Option<String>,
    series: Vec<String>,
    commit: git2::Oid,
    base: git2::Oid,
}

impl Change {
    pub fn new(_repo: &git2::Repository, commit: &git2::Commit) -> Self {
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

    pub fn set_base(&mut self, base: git2::Oid) {
        self.base = base;
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

pub fn encode_change_id_path(id: &str) -> String {
    id.replace('/', "%2F")
}

fn decode_change_id_path(enc: &str) -> String {
    enc.replace("%2F", "/")
}

/// Create a real merge commit that has the target branch tip and PR head as its two parents.
/// The tree is the PR head's tree (no content merge needed).
/// Author and committer are copied from the PR head commit.
pub fn create_synthetic_merge_commit(
    repo: &git2::Repository,
    pr_head: &git2::Commit,
    target_branch_tip: &git2::Commit,
    message: &str,
) -> anyhow::Result<git2::Oid> {
    let tree = pr_head.tree()?;
    let author = pr_head.author();
    let committer = pr_head.committer();

    let oid = repo.commit(
        None,
        &author,
        &committer,
        message,
        &tree,
        &[target_branch_tip, pr_head],
    )?;

    Ok(oid)
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
    transaction: &josh_core::cache::Transaction,
    changes: std::collections::HashMap<git2::Oid, Change>,
) -> anyhow::Result<Vec<Change>> {
    if changes.values().next().map(|c| c.base) == Some(git2::Oid::zero()) {
        return Ok(changes.into_values().collect());
    }

    changes
        .into_values()
        .map(|c| {
            let filter = josh_core::filter::Filter::new().downstack(c.base);
            let commit = repo.find_commit(c.commit)?;
            let new_oid = josh_core::filter::apply_to_commit(filter, &commit, transaction)?;
            let mut result = c;
            result.commit = new_oid;
            Ok(result)
        })
        .collect()
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
    transaction: &josh_core::cache::Transaction,
    push_mode: &PushMode,
    baseref: &str,
    ref_with_options: &str,
    oid_to_push: git2::Oid,
    base_oid: git2::Oid,
) -> anyhow::Result<Vec<PushRef>> {
    match push_mode {
        PushMode::Publish(author) => {
            let changes = get_changes(repo, oid_to_push, base_oid)?;
            let changes = split_changes(repo, transaction, changes)?;

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

pub fn sync_changes(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    tip: git2::Oid,
    base: git2::Oid,
) -> anyhow::Result<Vec<Change>> {
    let changes = get_changes(repo, tip, base)?;
    let changes = split_changes(repo, transaction, changes)?;
    for c in &changes {
        let _ = store_diff_data(repo, c);
    }
    Ok(changes)
}

pub fn list_changes(repo: &git2::Repository) -> anyhow::Result<Vec<Change>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Vec::new()),
    };

    let diffs_tree = match tree
        .get_name("diffs")
        .and_then(|e| e.to_object(repo).ok())
        .and_then(|o| o.peel_to_tree().ok())
    {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    let mut changes = Vec::new();
    for entry in diffs_tree.iter() {
        let change_id = decode_change_id_path(entry.name().unwrap_or(""));
        if change_id.is_empty() {
            continue;
        }
        let subtree = match entry
            .to_object(repo)
            .ok()
            .and_then(|o| o.peel_to_tree().ok())
        {
            Some(t) => t,
            None => continue,
        };
        // The subtree has a single blob named by its content hash.
        // Read it to get tip and base OIDs.
        let mut tip_oid = git2::Oid::zero();
        let mut base_oid = git2::Oid::zero();
        for se in subtree.iter() {
            let blob = match se.to_object(repo).ok().and_then(|o| o.peel_to_blob().ok()) {
                Some(b) => b,
                None => continue,
            };
            let content = String::from_utf8_lossy(blob.content());
            if let Some((tip_str, base_str)) = content.split_once('\n') {
                tip_oid = git2::Oid::from_str(tip_str).unwrap_or(git2::Oid::zero());
                base_oid = git2::Oid::from_str(base_str).unwrap_or(git2::Oid::zero());
            }
            break;
        }
        if tip_oid == git2::Oid::zero() {
            continue;
        }
        let commit = match repo.find_commit(tip_oid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut change = Change::new(repo, &commit);
        change.base = base_oid;
        if change.id.is_none() {
            change.id = Some(change_id);
        }
        changes.push(change);
    }
    Ok(changes)
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
    #[serde(skip)]
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
    write_comment_with_commit(repo, change, meta, author, timestamp, None)
}

pub fn write_comment_with_commit(
    repo: &git2::Repository,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    blob_commit_override: Option<&str>,
) -> anyhow::Result<String> {
    if meta.message.trim().is_empty() {
        return Err(anyhow::anyhow!("comment message must not be empty"));
    }

    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let content = serde_json::to_string(meta)?;
    let content_hash =
        git2::Oid::hash_object(git2::ObjectType::Blob, content.as_bytes())?.to_string();
    let blob_oid = repo.blob(content.as_bytes())?;

    let path = if let Some(ref file) = meta.file {
        let resolve_commit = match blob_commit_override {
            Some(s) => git2::Oid::from_str(s)?,
            None => change.commit(),
        };
        let commit = repo.find_commit(resolve_commit)?;
        let file_blob = commit
            .tree()?
            .get_path(std::path::Path::new(file))?
            .id()
            .to_string();
        std::path::Path::new("comments")
            .join("F")
            .join(encode_change_id_path(&change_id))
            .join(&file_blob)
            .join(file)
            .join(&content_hash)
    } else {
        std::path::Path::new("comments")
            .join("C")
            .join(encode_change_id_path(&change_id))
            .join(&content_hash)
    };
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
    pub author: Option<String>,
    pub timestamp: Option<String>,
}

pub fn comment_author(
    repo: &git2::Repository,
    change: &Change,
    comment_id: &str,
    file: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Err(anyhow!("change has no Change-Id")),
    };

    let head = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_commit()?,
        Err(_) => return Err(anyhow!("refs/josh/changes not found")),
    };

    let path = if let Some(f) = file {
        // Find the blob_id for this comment in the current tree.
        let head_tree = head.tree()?;
        let cid_path = std::path::Path::new("comments")
            .join("F")
            .join(encode_change_id_path(change_id));
        let mut found = None;
        if let Some(cid_tree) = get_tree(repo, &head_tree, &cid_path) {
            for blob_entry in cid_tree.iter() {
                let blob_name = blob_entry.name().unwrap_or("");
                let sub = std::path::Path::new(f).join(comment_id);
                let full = cid_path.join(blob_name).join(&sub);
                if head_tree.get_path(&full).is_ok() {
                    found = Some(full);
                    break;
                }
            }
        }
        match found {
            Some(p) => p,
            None => {
                return Err(anyhow!(
                    "comment {} not found in refs/josh/changes",
                    comment_id
                ));
            }
        }
    } else {
        std::path::Path::new("comments")
            .join("C")
            .join(encode_change_id_path(change_id))
            .join(comment_id)
    };

    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.push(head.id())?;

    for oid in walk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;
        if let Ok(entry) = tree.get_path(&path) {
            // Check if this blob is new (not in parent) or changed.
            let is_new = match commit.parent(0) {
                Ok(parent) => parent
                    .tree()
                    .ok()
                    .and_then(|pt| pt.get_path(&path).ok())
                    .map_or(true, |e| e.id() != entry.id()),
                Err(_) => true,
            };
            if is_new {
                let time = commit.time();
                let date = format!("{}", time.seconds());
                return Ok((commit.author().email().unwrap_or("").to_string(), date));
            }
        }
    }

    Err(anyhow!(
        "comment {} not found in refs/josh/changes",
        comment_id
    ))
}

fn get_tree<'a>(
    repo: &'a git2::Repository,
    tree: &'a git2::Tree,
    path: &std::path::Path,
) -> Option<git2::Tree<'a>> {
    tree.get_path(path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
        .and_then(|o| o.peel_to_tree().ok())
}

fn parse_comment_blob(
    repo: &git2::Repository,
    entry: &git2::TreeEntry,
    file: Option<String>,
) -> anyhow::Result<Comment> {
    let id = entry.name().unwrap_or("").to_string();
    let blob = entry.to_object(repo)?.peel_to_blob()?;
    let meta: CommentMeta = serde_json::from_slice(blob.content())?;
    Ok(Comment {
        id,
        message: meta.message,
        file,
        location: meta.location,
        reply_to: meta.reply_to,
        update_of: meta.update_of,
        author: None,
        timestamp: None,
    })
}

fn collect_comments_under(
    repo: &git2::Repository,
    tree: &git2::Tree,
    file_prefix: &std::path::Path,
) -> anyhow::Result<Vec<Comment>> {
    let mut comments = Vec::new();
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                let subtree = entry.to_object(repo)?.peel_to_tree()?;
                let child_file = file_prefix.join(name);
                comments.extend(collect_comments_under(repo, &subtree, &child_file)?);
            }
            Some(git2::ObjectType::Blob) => {
                let file = if file_prefix.as_os_str().is_empty() {
                    None
                } else {
                    Some(file_prefix.to_string_lossy().to_string())
                };
                comments.push(parse_comment_blob(repo, &entry, file)?);
            }
            _ => {}
        }
    }
    Ok(comments)
}

pub fn read_comments(repo: &git2::Repository, change_id: &str) -> anyhow::Result<Vec<Comment>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Vec::new()),
    };

    let comments_tree = match tree.get_name("comments") {
        Some(e) => e.to_object(repo)?.peel_to_tree()?,
        None => return Ok(Vec::new()),
    };

    let mut comments = Vec::new();

    // Non-file comments: comments/C/<change_id>/
    if let Some(cid_tree) = get_tree(
        repo,
        &comments_tree,
        &std::path::Path::new("C").join(encode_change_id_path(change_id)),
    ) {
        for entry in cid_tree.iter() {
            if let Ok(c) = parse_comment_blob(repo, &entry, None) {
                comments.push(c);
            }
        }
    }

    // File comments: comments/F/<change_id>/<blob_id>/<path>/<to>/<file>/<content_hash>
    if let Some(cid_tree) = get_tree(
        repo,
        &comments_tree,
        &std::path::Path::new("F").join(encode_change_id_path(change_id)),
    ) {
        for blob_entry in cid_tree.iter() {
            let blob_name = blob_entry.name().unwrap_or("");
            if let Some(blob_tree) = get_tree(repo, &cid_tree, std::path::Path::new(blob_name)) {
                comments.extend(collect_comments_under(
                    repo,
                    &blob_tree,
                    std::path::Path::new(""),
                )?);
            }
        }
    }

    // Walk history once to resolve author/timestamp for all comments.
    if let Ok(head) = repo.find_reference("refs/josh/changes") {
        if let Ok(head_commit) = head.peel_to_commit() {
            let mut walk = repo.revwalk().unwrap_or_else(|_| repo.revwalk().unwrap());
            let _ = walk.simplify_first_parent();
            let _ = walk.push(head_commit.id());
            'outer: for oid in walk.flatten() {
                if let Ok(commit) = repo.find_commit(oid) {
                    let tree = commit.tree().unwrap();
                    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
                    for c in &mut comments {
                        if c.author.is_some() {
                            continue;
                        }
                        if c.file.is_none() {
                            let p = std::path::Path::new("comments")
                                .join("C")
                                .join(encode_change_id_path(change_id))
                                .join(&c.id);
                            if let Ok(entry) = tree.get_path(&p) {
                                let is_new = parent_tree
                                    .as_ref()
                                    .and_then(|pt| pt.get_path(&p).ok())
                                    .map_or(true, |e| e.id() != entry.id());
                                if is_new {
                                    let time = commit.time();
                                    let ts = time.seconds().to_string();
                                    c.author =
                                        Some(commit.author().email().unwrap_or("").to_string());
                                    c.timestamp = Some(ts);
                                }
                            }
                        } else {
                            let cid_path = std::path::Path::new("comments")
                                .join("F")
                                .join(encode_change_id_path(change_id));
                            if let Some(cid_tree) = get_tree(repo, &tree, &cid_path) {
                                for blob_entry in cid_tree.iter() {
                                    let blob_name = blob_entry.name().unwrap_or("");
                                    let sub =
                                        std::path::Path::new(c.file.as_ref().unwrap()).join(&c.id);
                                    let full_path = cid_path.join(blob_name).join(&sub);
                                    if let Ok(entry) = tree.get_path(&full_path) {
                                        let parent_entry = parent_tree
                                            .as_ref()
                                            .and_then(|pt| pt.get_path(&full_path).ok());
                                        let is_new =
                                            parent_entry.map_or(true, |e| e.id() != entry.id());
                                        if is_new {
                                            let time = commit.time();
                                            let ts = time.seconds().to_string();
                                            c.author = Some(
                                                commit.author().email().unwrap_or("").to_string(),
                                            );
                                            c.timestamp = Some(ts);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if comments.iter().all(|c| c.author.is_some()) {
                        break 'outer;
                    }
                }
            }
        }
    }

    Ok(comments)
}

#[derive(Debug, Clone)]
pub struct Revision {
    pub commit_oid: String,
    pub author: String,
    pub timestamp: String,
}

pub fn read_revisions(repo: &git2::Repository, change: &Change) -> anyhow::Result<Vec<Revision>> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    let head = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_commit()?,
        Err(_) => return Ok(Vec::new()),
    };

    let mut revs: Vec<Revision> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.push(head.id())?;

    for oid in walk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let tree = match commit.parent(0) {
            Ok(p) => (p.tree().ok(), commit.tree().ok()),
            Err(_) => (None, commit.tree().ok()),
        };
        let (parent_tree, cur_tree) = tree;
        let cur_tree = match cur_tree {
            Some(t) => t,
            None => continue,
        };

        let diffs_tree = match cur_tree
            .get_name("diffs")
            .and_then(|e| e.to_object(repo).ok())
            .and_then(|o| o.peel_to_tree().ok())
        {
            Some(t) => t,
            None => continue,
        };
        let cid_tree = match diffs_tree
            .get_name(&encode_change_id_path(change_id))
            .and_then(|e| e.to_object(repo).ok())
            .and_then(|o| o.peel_to_tree().ok())
        {
            Some(t) => t,
            None => continue,
        };

        let parent_cid_tree = parent_tree.as_ref().and_then(|pt| {
            let diffs = pt
                .get_name("diffs")?
                .to_object(repo)
                .ok()?
                .peel_to_tree()
                .ok()?;
            let cid = diffs
                .get_name(&encode_change_id_path(change_id))?
                .to_object(repo)
                .ok()?
                .peel_to_tree()
                .ok()?;
            Some(cid)
        });

        for entry in cid_tree.iter() {
            let commit_oid = entry.name().unwrap_or("").to_string();
            if commit_oid.is_empty() || seen.contains(&commit_oid) {
                continue;
            }
            let is_new = parent_cid_tree
                .as_ref()
                .and_then(|pt| pt.get_name(&commit_oid))
                .map_or(true, |e| e.id() != entry.id());
            if !is_new {
                continue;
            }
            let time = commit.time();
            seen.insert(commit_oid.clone());
            revs.push(Revision {
                commit_oid,
                author: commit.author().email().unwrap_or("").to_string(),
                timestamp: time.seconds().to_string(),
            });
        }
    }

    revs.reverse();
    Ok(revs)
}

pub fn store_diff_data(repo: &git2::Repository, change: &Change) -> anyhow::Result<()> {
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let commit_oid_str = change.commit().to_string();
    let base_str = change.base().to_string();
    let content = format!("{}\n{}", commit_oid_str, base_str);
    let blob_oid = repo.blob(content.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    let entry_name = blob_oid.to_string();
    tb.insert(&entry_name, blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let base_tree = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    let path = std::path::Path::new("diffs").join(encode_change_id_path(&change_id));

    if let Some(existing) = base_tree
        .get_path(&path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == tree_oid {
            return Ok(());
        }
    }

    let tree = josh_core::filter::tree::insert(repo, &base_tree, &path, tree_oid, 0o0040000)?;

    let sig = repo.signature()?;
    let prev_tip = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_commit().ok());

    let source_commit = repo.find_commit(change.commit())?;
    let anchor_sig = git2::Signature::new("JOSH", "josh@josh-project.dev", &git2::Time::new(0, 0))?;
    let empty_tree = repo.find_tree(josh_core::filter::tree::empty_id())?;
    let anchor_oid = repo.commit(
        None,
        &anchor_sig,
        &anchor_sig,
        "josh\n",
        &empty_tree,
        &[&source_commit],
    )?;
    let anchor_commit = repo.find_commit(anchor_oid)?;

    let mut parents: Vec<&git2::Commit> = Vec::new();
    if let Some(ref c) = prev_tip {
        parents.push(c);
    }
    parents.push(&anchor_commit);
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

pub fn store_pr_data(repo: &git2::Repository, change_id: &str, json: &str) -> anyhow::Result<()> {
    let blob_oid = repo.blob(json.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    tb.insert(&blob_oid.to_string(), blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let base_tree = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    let path = std::path::Path::new("gh").join(encode_change_id_path(change_id));

    if let Some(existing) = base_tree
        .get_path(&path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == tree_oid {
            return Ok(());
        }
    }

    let tree = josh_core::filter::tree::insert(repo, &base_tree, &path, tree_oid, 0o0040000)?;

    let sig = repo.signature()?;
    let prev_tip = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = prev_tip.iter().collect();
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

/// Read stored GitHub PR data JSON for a change, if it exists.
pub fn read_pr_data(repo: &git2::Repository, change_id: &str) -> anyhow::Result<Option<String>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(None),
    };
    let gh_path = std::path::Path::new("gh").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &gh_path) {
        Some(t) => t,
        None => return Ok(None),
    };
    for entry in subtree.iter() {
        if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
            return Ok(Some(String::from_utf8_lossy(blob.content()).to_string()));
        }
    }
    Ok(None)
}

/// Delete all stored data for a change from refs/josh/changes.
/// Removes entries from diffs/, comments/C/, comments/F/, gh/, and gh_ids/ subtrees.
pub fn delete_change(repo: &git2::Repository, change_id: &str) -> anyhow::Result<()> {
    let encoded = encode_change_id_path(change_id);

    let base_tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(()),
    };

    let mut tree = base_tree;
    for prefix in &[
        "diffs",
        "comments/C",
        "comments/F",
        "gh",
        "gh_ids",
        "votes",
        "gh_vote_ids",
    ] {
        let path = std::path::Path::new(prefix).join(&encoded);
        if tree.get_path(&path).is_ok() {
            tree = josh_core::filter::tree::insert(repo, &tree, &path, git2::Oid::zero(), 0)?;
        }
    }

    let sig = repo.signature()?;
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

/// Store a GitHub node ID for a local comment, marking it as posted.
pub fn store_github_id(
    repo: &git2::Repository,
    change_id: &str,
    local_hash: &str,
    github_id: &str,
) -> anyhow::Result<()> {
    let blob_oid = repo.blob(github_id.as_bytes())?;
    let path = std::path::Path::new("gh_ids")
        .join(encode_change_id_path(change_id))
        .join(local_hash);
    write_changes_tree(repo, &path, blob_oid, None, None)?;
    Ok(())
}

/// Read all GitHub node IDs for a change's comments.
/// Returns a map from local comment hash → GitHub node ID.
pub fn read_github_ids(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Default::default()),
    };
    let gh_ids_path = std::path::Path::new("gh_ids").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &gh_ids_path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut map = std::collections::HashMap::new();
    for entry in subtree.iter() {
        if let Some(name) = entry.name() {
            if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
                let github_id = String::from_utf8_lossy(blob.content()).trim().to_string();
                map.insert(name.to_string(), github_id);
            }
        }
    }
    Ok(map)
}

pub fn store_github_vote_id(
    repo: &git2::Repository,
    change_id: &str,
    user: &str,
    vote_data: &VoteData,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(vote_data)?;
    let blob_oid = repo.blob(json.as_bytes())?;
    let path = std::path::Path::new("gh_vote_ids")
        .join(encode_change_id_path(change_id))
        .join(user);
    write_changes_tree(repo, &path, blob_oid, None, None)?;
    Ok(())
}

pub fn read_github_vote_ids(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<std::collections::HashMap<String, VoteData>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Default::default()),
    };
    let path = std::path::Path::new("gh_vote_ids").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut map = std::collections::HashMap::new();
    for entry in subtree.iter() {
        if let Some(user) = entry.name() {
            if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
                if let Ok(data) = serde_json::from_slice::<VoteData>(blob.content()) {
                    map.insert(user.to_string(), data);
                }
            }
        }
    }
    Ok(map)
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteData {
    pub state: String,
    pub sha: String,
}

pub fn vote_state_to_github_review(state: &str) -> &'static str {
    match state {
        "approve" => "APPROVE",
        "discuss" => "COMMENT",
        "revise" => "REQUEST_CHANGES",
        _ => "COMMENT",
    }
}

pub fn write_vote(
    repo: &git2::Repository,
    change: &Change,
    state: &str,
    author: Option<&str>,
    timestamp: Option<&str>,
) -> anyhow::Result<String> {
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let json = serde_json::json!({"state": state, "sha": change.commit().to_string()});
    let content = json.to_string();
    let content_hash =
        git2::Oid::hash_object(git2::ObjectType::Blob, content.as_bytes())?.to_string();
    let blob_oid = repo.blob(content.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    tb.insert(&blob_oid.to_string(), blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let user = match author {
        Some(name) => name.to_string(),
        None => repo.signature()?.email().unwrap_or("unknown").to_string(),
    };

    let path = std::path::Path::new("votes")
        .join(encode_change_id_path(&change_id))
        .join(&user);

    let base_tree = repo
        .find_reference("refs/josh/changes")
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    if let Some(existing) = base_tree
        .get_path(&path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == tree_oid {
            return Ok(content_hash);
        }
    }

    let tree = josh_core::filter::tree::insert(repo, &base_tree, &path, tree_oid, 0o0040000)?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => repo.signature()?,
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

    Ok(content_hash)
}

pub fn read_vote(
    repo: &git2::Repository,
    change_id: &str,
    user: Option<&str>,
) -> anyhow::Result<Option<VoteData>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(None),
    };

    let user = match user {
        Some(name) => name.to_string(),
        None => repo.signature()?.email().unwrap_or("unknown").to_string(),
    };

    let path = std::path::Path::new("votes")
        .join(encode_change_id_path(change_id))
        .join(&user);

    let subtree = match get_tree(repo, &tree, &path) {
        Some(t) => t,
        None => return Ok(None),
    };
    for entry in subtree.iter() {
        if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
            let data: VoteData = serde_json::from_slice(blob.content())?;
            return Ok(Some(data));
        }
    }
    Ok(None)
}

pub fn list_votes(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let tree = match repo.find_reference("refs/josh/changes") {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Default::default()),
    };
    let path = std::path::Path::new("votes").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut votes = Vec::new();
    for entry in subtree.iter() {
        let user = match entry.name() {
            Some(name) => name.to_string(),
            None => continue,
        };
        let user_tree = match entry.to_object(repo).and_then(|o| o.peel_to_tree()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for child in user_tree.iter() {
            if let Ok(blob) = child.to_object(repo).and_then(|o| o.peel_to_blob()) {
                if let Ok(data) = serde_json::from_slice::<VoteData>(blob.content()) {
                    votes.push((user.clone(), data));
                }
            }
        }
    }
    Ok(votes)
}
