use josh_changes::{Change, PushRef, get_changes, split_changes};

/// A valid Gerrit Change-Id is the letter `I` followed by 40 hex digits.
fn is_gerrit_change_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let Some((prefix, rest)) = bytes.split_first() else {
        return false;
    };
    *prefix == b'I' && rest.len() == 40 && rest.iter().all(u8::is_ascii_hexdigit)
}

/// Map a josh change id to a Gerrit-format Change-Id (`I` + 40 hex).
///
/// If the josh id already looks like a Gerrit Change-Id it is used verbatim.
/// Otherwise the id is derived deterministically by hashing the josh id, so
/// re-publishing an unchanged change lands as a new patchset on the *same*
/// Gerrit change rather than creating a duplicate.
fn gerrit_change_id(josh_id: &str) -> anyhow::Result<String> {
    if is_gerrit_change_id(josh_id) {
        return Ok(josh_id.to_string());
    }
    let oid = josh_core::objects::hash_blob(josh_id.as_bytes());
    Ok(format!("I{}", oid))
}

/// Return `message` with any existing `Change:`/`Change-Id:` trailer replaced by
/// a single `Change-Id: <gerrit_id>` trailer in the footer block.
fn message_with_gerrit_change_id(message: &str, gerrit_id: &str) -> String {
    let lines: Vec<&str> = message.lines().collect();

    // Find the footer block the same way `parse_change_meta` does.
    let mut footer_start = lines.len();
    for (i, line) in lines.iter().enumerate().rev() {
        if line.is_empty() || josh_core::trailers::is_trailer_line(line) {
            footer_start = i;
        } else {
            break;
        }
    }

    let mut kept: Vec<&str> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i >= footer_start && (line.starts_with("Change: ") || line.starts_with("Change-Id: ")) {
            continue;
        }
        kept.push(line);
    }
    while matches!(kept.last(), Some(l) if l.is_empty()) {
        kept.pop();
    }

    let trailer = format!("Change-Id: {}", gerrit_id);
    if kept.is_empty() {
        return trailer;
    }

    let mut body = kept.join("\n");
    if josh_core::trailers::is_trailer_line(kept.last().unwrap()) {
        // Extend the existing trailer block.
        body.push('\n');
    } else {
        // Start a fresh footer block after the message body.
        body.push_str("\n\n");
    }
    body.push_str(&trailer);
    body
}

/// Rewrite the linear first-parent chain `(base, tip]` so every commit carries a
/// deterministic Gerrit `Change-Id` trailer, and return the new tip oid.
///
/// Commits are rebuilt bottom-up with their trees preserved. The Change-Id is
/// derived from each commit's josh change id, so an unchanged change rewrites to
/// a stable id and re-publishing lands as a new patchset on the same Gerrit
/// change rather than a duplicate.
fn rewrite_chain_with_gerrit_ids(
    transaction: &josh_core::cache::Transaction,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();

    // Collect the chain from tip down to (but excluding) base, first parent only.
    let mut chain: Vec<gix_hash::ObjectId> = Vec::new();
    let mut cur = tip;
    while cur != base {
        chain.push(cur);
        match josh_core::objects::CommitData::read(odb, cur)?.first_parent_id() {
            Some(p) => cur = p,
            None => break,
        }
    }
    chain.reverse();

    let mut new_parent = (base != gix_hash::ObjectId::null(gix_hash::Kind::Sha1)).then_some(base);
    for oid in chain {
        let commit = josh_core::objects::CommitData::read(odb, oid)?;
        let (josh_id, _) = josh_core::trailers::commit_change_meta(&commit);
        // A commit with no josh change id gets one derived from its own oid: rare
        // for a change stack, but Gerrit requires a Change-Id on every commit.
        let gerrit_id = gerrit_change_id(josh_id.as_deref().unwrap_or(&oid.to_string()))?;
        let message = commit
            .message()
            .ok()
            .and_then(|m| std::str::from_utf8(m).ok())
            .unwrap_or("");
        let new_message = message_with_gerrit_change_id(message, &gerrit_id);

        new_parent = Some(josh_core::objects::write_commit_with_signatures_of(
            odb,
            &commit,
            commit.tree_id()?,
            new_parent.as_slice(),
            &new_message,
        )?);
    }

    Ok(new_parent.unwrap_or(base))
}

/// Build the ref to push when publishing a stacked change series to a Gerrit
/// server.
///
/// Unlike the GitHub path, which splits each change into an independent
/// `downstack` diff, Gerrit publishing pushes the **actual linear commit
/// history** `(base, tip]` once to `refs/for/<branch>`. Gerrit then makes one
/// change per commit and chains them into a relation chain by commit ancestry.
///
/// This is deliberate: Gerrit keys a change by its `Change-Id` and expects it to
/// appear exactly once with a single parent. The `downstack` model, where one
/// change can belong to several stacks with differing ancestry, cannot be
/// represented consistently in Gerrit -- so we push the real history instead,
/// where every change appears once with exactly one parent. Each commit is
/// rewritten to carry a deterministic Gerrit `Change-Id`.
pub fn build_gerrit_push(
    transaction: &josh_core::cache::Transaction,
    branch: &str,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
) -> anyhow::Result<Vec<PushRef>> {
    if tip == base {
        return Ok(vec![]);
    }

    let new_tip = rewrite_chain_with_gerrit_ids(transaction, tip, base)?;
    Ok(vec![PushRef {
        ref_name: format!("refs/for/{}", branch),
        oid: new_tip,
        change_id: "JOSH_GERRIT_PUSH".to_string(),
    }])
}

/// Build the refs to push when publishing to Gerrit in `independent` mode.
///
/// Only changes with **no dependencies** are pushed -- those whose `downstack`
/// decomposition sits directly on the target base. Each is pushed as its own
/// single-commit review on `refs/for/<branch>`, so it is independently
/// reviewable and submittable. Changes that still depend on unmerged work are
/// skipped; they become roots (and get published) once their dependencies
/// merge. Because only roots are pushed, no change ever appears inside another
/// change's stack, so there is no duplicated or conflicting ancestry.
///
/// The returned `PushRef`s all target the same `refs/for/<branch>` ref with
/// distinct oids; the caller pushes each in its own `git push` invocation.
pub fn build_gerrit_independent_push(
    transaction: &josh_core::cache::Transaction,
    branch: &str,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
) -> anyhow::Result<Vec<PushRef>> {
    let odb = transaction.odb();
    let changes = get_changes(transaction, tip, base)?;
    let changes = split_changes(transaction, changes)?;

    // `get_changes` collects into a HashMap, so sort for a deterministic push order.
    let mut changes: Vec<Change> = changes.into_iter().collect();
    changes.sort_by_key(|c| c.commit());

    let ref_name = format!("refs/for/{}", branch);
    let mut refs = Vec::new();
    for change in changes {
        let Some(josh_id) = change.id().map(|s| s.to_string()) else {
            continue;
        };

        // A change has no dependencies when its split commit sits directly on
        // the target base (nothing else was pulled in below it by `downstack`).
        let parent = josh_core::objects::CommitData::read(odb, change.commit())?
            .parent_ids()
            .next();
        let has_no_deps = if base == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
            parent.is_none()
        } else {
            parent == Some(base)
        };
        if !has_no_deps {
            continue;
        }

        let gerrit_id = gerrit_change_id(&josh_id)?;
        let new_tip = rewrite_chain_with_gerrit_ids(transaction, change.commit(), base)?;
        refs.push(PushRef {
            ref_name: ref_name.clone(),
            oid: new_tip,
            change_id: gerrit_id,
        });
    }
    Ok(refs)
}

#[cfg(test)]
mod gerrit_tests {
    use super::*;

    #[test]
    fn gerrit_change_id_is_deterministic_and_well_formed() {
        let a = gerrit_change_id("my-josh-change").unwrap();
        let b = gerrit_change_id("my-josh-change").unwrap();
        assert_eq!(a, b, "same josh id must map to the same Gerrit Change-Id");
        assert!(is_gerrit_change_id(&a), "must be I + 40 hex, got {a}");
        assert_ne!(
            a,
            gerrit_change_id("other-change").unwrap(),
            "different josh ids should not collide"
        );
    }

    #[test]
    fn gerrit_change_id_passes_through_valid_ids() {
        let existing = "I0123456789abcdef0123456789abcdef01234567";
        assert_eq!(gerrit_change_id(existing).unwrap(), existing);
    }

    #[test]
    fn is_gerrit_change_id_rejects_bad_shapes() {
        assert!(!is_gerrit_change_id("I0123")); // too short
        assert!(!is_gerrit_change_id(
            "0123456789abcdef0123456789abcdef01234567"
        )); // no leading I
        assert!(!is_gerrit_change_id(
            "Ixyz456789abcdef0123456789abcdef01234567"
        )); // non-hex
        assert!(!is_gerrit_change_id("my-josh-change"));
    }

    #[test]
    fn message_replaces_josh_change_trailer() {
        let msg = "Subject\n\nBody.\n\nChange: my-id\n";
        let out = message_with_gerrit_change_id(msg, "Iabc");
        assert_eq!(out, "Subject\n\nBody.\n\nChange-Id: Iabc");
    }

    #[test]
    fn message_keeps_other_trailers_in_block() {
        let msg = "Subject\n\nBody.\n\nSigned-off-by: a <a@b>\nChange-Id: old\n";
        let out = message_with_gerrit_change_id(msg, "Inew");
        assert_eq!(
            out,
            "Subject\n\nBody.\n\nSigned-off-by: a <a@b>\nChange-Id: Inew"
        );
    }

    #[test]
    fn message_appends_footer_when_none_present() {
        let msg = "Just a subject";
        let out = message_with_gerrit_change_id(msg, "Iabc");
        assert_eq!(out, "Just a subject\n\nChange-Id: Iabc");
    }

    #[test]
    fn message_from_single_change_trailer_line() {
        let out = message_with_gerrit_change_id("Change: only-line", "Iabc");
        assert_eq!(out, "Change-Id: Iabc");
    }
}
