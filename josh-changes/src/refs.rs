//! Typed model of the refs josh publishes for a change stack.
//!
//! Three ref families exist on the remote:
//!
//! - `refs/heads/@changes/<target>/<author>/<change-id>` — head commit of a change
//! - `refs/heads/@base/<target>/<author>/<change-id>` — commit the change is based on
//! - `refs/heads/@heads/<target>/<author>` — tip of the developer's published stack
//!
//! `<target>` is a branch name and may contain `/`. `<author>` is an email
//! address; it is located during parsing as the first `/`-separated segment
//! containing `@`, which is also why change-ids containing `@` are rejected at
//! push time (see `changes_to_refs`). `<change-id>` may itself contain `/`
//! (e.g. synthetic `owner/repo/pull/N` ids), so everything after the author
//! segment is treated as the change-id.

use anyhow::anyhow;

/// A change-scoped ref: either the change's head commit (`Change`) or the
/// commit it is based on (`Base`). Both spellings carry the same identity, so
/// they convert freely via [`StackedChangeRef::as_change`] and
/// [`StackedChangeRef::as_base`].
///
/// This type is pure data; the ref-name grammar lives on [`StackedRef`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StackedChangeRef {
    Change {
        target: String,
        author: String,
        change_id: String,
    },
    Base {
        target: String,
        author: String,
        change_id: String,
    },
}

impl StackedChangeRef {
    /// The same change, spelled as its `@changes` ref.
    pub fn as_change(&self) -> StackedChangeRef {
        match self {
            StackedChangeRef::Change { .. } => self.clone(),
            StackedChangeRef::Base {
                target,
                author,
                change_id,
            } => StackedChangeRef::Change {
                target: target.clone(),
                author: author.clone(),
                change_id: change_id.clone(),
            },
        }
    }

    /// The same change, spelled as its `@base` ref.
    pub fn as_base(&self) -> StackedChangeRef {
        match self {
            StackedChangeRef::Base { .. } => self.clone(),
            StackedChangeRef::Change {
                target,
                author,
                change_id,
            } => StackedChangeRef::Base {
                target: target.clone(),
                author: author.clone(),
                change_id: change_id.clone(),
            },
        }
    }

    pub fn target(&self) -> &str {
        match self {
            StackedChangeRef::Change { target, .. } | StackedChangeRef::Base { target, .. } => {
                target
            }
        }
    }

    pub fn author(&self) -> &str {
        match self {
            StackedChangeRef::Change { author, .. } | StackedChangeRef::Base { author, .. } => {
                author
            }
        }
    }

    pub fn change_id(&self) -> &str {
        match self {
            StackedChangeRef::Change { change_id, .. }
            | StackedChangeRef::Base { change_id, .. } => change_id,
        }
    }
}

/// Any ref josh publishes for a change stack. Owns the ref-name grammar:
/// the only place that knows the `@changes` / `@base` / `@heads` spellings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StackedRef {
    /// A `@changes/...` or `@base/...` ref.
    ChangeRef(StackedChangeRef),
    /// A `@heads/<target>/<author>` ref: tip of a developer's published stack.
    StackHead { target: String, author: String },
}

impl StackedRef {
    /// The full `refs/heads/@...` name of this ref.
    pub fn ref_name(&self) -> String {
        match self {
            StackedRef::ChangeRef(StackedChangeRef::Change {
                target,
                author,
                change_id,
            }) => format!("refs/heads/@changes/{}/{}/{}", target, author, change_id),
            StackedRef::ChangeRef(StackedChangeRef::Base {
                target,
                author,
                change_id,
            }) => format!("refs/heads/@base/{}/{}/{}", target, author, change_id),
            StackedRef::StackHead { target, author } => {
                format!("refs/heads/@heads/{}/{}", target, author)
            }
        }
    }

    /// Parse a stacked-changes ref name. Accepts the full `refs/heads/@...`
    /// form, the remote-tracking `refs/remotes/<remote>/@...` form, and the
    /// bare `@...` shorthand. Returns `None` for refs outside the three
    /// families.
    pub fn parse(ref_name: &str) -> Option<StackedRef> {
        let name = if let Some(rest) = ref_name.strip_prefix("refs/heads/") {
            rest
        } else if let Some(rest) = ref_name.strip_prefix("refs/remotes/") {
            // Skip the remote name (cannot contain '/').
            rest.split_once('/')?.1
        } else {
            ref_name
        };

        let (is_base, rest) = if let Some(rest) = name.strip_prefix("@changes/") {
            (false, rest)
        } else if let Some(rest) = name.strip_prefix("@base/") {
            (true, rest)
        } else if let Some(rest) = name.strip_prefix("@heads/") {
            let (target, author, trailing) = split_target_author(rest)?;
            if !trailing.is_empty() {
                return None;
            }
            return Some(StackedRef::StackHead { target, author });
        } else {
            return None;
        };

        let (target, author, change_id) = split_target_author(rest)?;
        if change_id.is_empty() {
            return None;
        }
        let change = StackedChangeRef::Change {
            target,
            author,
            change_id,
        };
        Some(StackedRef::ChangeRef(if is_base {
            change.as_base()
        } else {
            change
        }))
    }
}

/// Split `<target>/<author>[/<change-id>]` into its three parts. The author
/// is the first `/`-separated segment containing `@`; the target is
/// everything before it, and the change-id (possibly empty, possibly
/// containing `/`) is everything after it.
fn split_target_author(rest: &str) -> Option<(String, String, String)> {
    let mut target_end = 0;
    for segment in rest.split('/') {
        if segment.contains('@') {
            if target_end == 0 {
                return None;
            }
            let target = &rest[..target_end - 1];
            let after_author = &rest[target_end + segment.len()..];
            let change_id = after_author.strip_prefix('/').unwrap_or(after_author);
            return Some((
                target.to_string(),
                segment.to_string(),
                change_id.to_string(),
            ));
        }
        target_end += segment.len() + 1;
    }
    None
}

/// Which `refs/josh/...` ref holds a piece of change metadata.
///
/// Scoped by the target branch of the change, so a single repo can host
/// changes against multiple branches without collisions.
///
/// `Local { branch }` is data the user authored or discovered from their
/// working tree, targeting `branch`.
/// `Remote { remote, branch }` is data fetched from / posted to `remote`
/// for changes targeting `branch`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChangesRef {
    Local { branch: String },
    Remote { remote: String, branch: String },
}

impl ChangesRef {
    /// Select the changes ref a command should operate on.
    ///
    /// `branch` defaults to the current HEAD's branch (see [`head_branch`]).
    /// `remote` selects the `Remote` variant for that remote; `None` selects
    /// the `Local` variant.
    pub fn resolve(
        transaction: &josh_core::cache::Transaction,
        branch: Option<&str>,
        remote: Option<&str>,
    ) -> anyhow::Result<ChangesRef> {
        let branch = match branch {
            Some(b) => b.to_string(),
            None => head_branch(transaction)?,
        };
        Ok(match remote {
            Some(remote) => ChangesRef::Remote {
                remote: remote.to_string(),
                branch,
            },
            None => ChangesRef::Local { branch },
        })
    }

    pub fn ref_name(&self) -> String {
        match self {
            ChangesRef::Local { branch } => format!("refs/josh/changes/{}", branch),
            ChangesRef::Remote { remote, branch } => {
                format!("refs/josh/remotes/{}/changes/{}", remote, branch)
            }
        }
    }

    pub fn branch(&self) -> &str {
        match self {
            ChangesRef::Local { branch } => branch,
            ChangesRef::Remote { branch, .. } => branch,
        }
    }

    pub fn remote(&self) -> Option<&str> {
        match self {
            ChangesRef::Local { .. } => None,
            ChangesRef::Remote { remote, .. } => Some(remote),
        }
    }
}

/// Return the changes ref's target OID, if it exists.
pub fn read_ref_oid(
    transaction: &josh_core::cache::Transaction,
    scope: &ChangesRef,
) -> Option<gix_hash::ObjectId> {
    transaction.resolve_ref(&scope.ref_name()).ok().flatten()
}

/// Read HEAD and return the current branch shorthand. Errors on a
/// detached HEAD with a message asking the caller to pass an explicit branch.
pub fn head_branch(transaction: &josh_core::cache::Transaction) -> anyhow::Result<String> {
    let head = transaction
        .head()
        .map_err(|e| anyhow!("failed to read HEAD: {}", e))?;
    let branch = head.short_branch().ok_or_else(|| {
        anyhow!("HEAD is detached -- pass --branch to select a target branch explicitly")
    })?;
    Ok(branch.to_string())
}

/// Return every changes ref that currently exists, as `ChangesRef` values.
/// `(remote, branch)` pairs are sorted for deterministic iteration; Local
/// entries come first.
pub fn all_changes_refs(
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<Vec<ChangesRef>> {
    let mut locals: Vec<String> = Vec::new();
    let mut remotes: Vec<(String, String)> = Vec::new();
    transaction.for_each_ref_prefixed("refs/josh/changes/", |name, _| {
        let branch = &name["refs/josh/changes/".len()..];
        if !branch.is_empty() {
            locals.push(branch.to_string());
        }
        Ok(())
    })?;
    transaction.for_each_ref_prefixed("refs/josh/remotes/", |name, _| {
        // `<remote>/changes/<branch>` -- branch may contain `/`, so split
        // on the literal `/changes/` separator.
        let rest = &name["refs/josh/remotes/".len()..];
        if let Some((remote, branch)) = rest.split_once("/changes/") {
            if !remote.is_empty() && !branch.is_empty() {
                remotes.push((remote.to_string(), branch.to_string()));
            }
        }
        Ok(())
    })?;
    locals.sort();
    locals.dedup();
    remotes.sort();
    remotes.dedup();

    let mut out = Vec::with_capacity(locals.len() + remotes.len());
    for branch in locals {
        out.push(ChangesRef::Local { branch });
    }
    for (remote, branch) in remotes {
        out.push(ChangesRef::Remote { remote, branch });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change() -> StackedChangeRef {
        StackedChangeRef::Change {
            target: "master".to_string(),
            author: "josh@example.com".to_string(),
            change_id: "1234".to_string(),
        }
    }

    #[test]
    fn ref_name_round_trips() {
        for stacked in [
            StackedRef::ChangeRef(change()),
            StackedRef::ChangeRef(change().as_base()),
            StackedRef::StackHead {
                target: "master".to_string(),
                author: "josh@example.com".to_string(),
            },
        ] {
            assert_eq!(StackedRef::parse(&stacked.ref_name()), Some(stacked));
        }
    }

    #[test]
    fn base_change_conversion_is_lossless() {
        let c = change();
        assert_eq!(c.as_base().as_change(), c);
        let b = c.as_base();
        assert_eq!(b.as_change().as_base(), b);
        assert_eq!(
            StackedRef::ChangeRef(c.as_base()).ref_name(),
            "refs/heads/@base/master/josh@example.com/1234"
        );
    }

    #[test]
    fn parses_shorthand_and_remote_forms() {
        assert_eq!(
            StackedRef::parse("@changes/master/josh@example.com/1234"),
            Some(StackedRef::ChangeRef(change()))
        );
        assert_eq!(
            StackedRef::parse("refs/remotes/origin/@changes/master/josh@example.com/1234"),
            Some(StackedRef::ChangeRef(change()))
        );
        assert_eq!(
            StackedRef::parse("refs/remotes/origin/@heads/master/josh@example.com"),
            Some(StackedRef::StackHead {
                target: "master".to_string(),
                author: "josh@example.com".to_string(),
            })
        );
    }

    #[test]
    fn parses_target_with_slashes() {
        assert_eq!(
            StackedRef::parse("refs/heads/@changes/feature/foo/josh@example.com/1234"),
            Some(StackedRef::ChangeRef(StackedChangeRef::Change {
                target: "feature/foo".to_string(),
                author: "josh@example.com".to_string(),
                change_id: "1234".to_string(),
            }))
        );
    }

    #[test]
    fn parses_change_id_with_slashes() {
        assert_eq!(
            StackedRef::parse("refs/heads/@changes/master/josh@example.com/o/r/pull/1"),
            Some(StackedRef::ChangeRef(StackedChangeRef::Change {
                target: "master".to_string(),
                author: "josh@example.com".to_string(),
                change_id: "o/r/pull/1".to_string(),
            }))
        );
    }

    #[test]
    fn rejects_non_stacked_refs() {
        assert_eq!(StackedRef::parse("refs/heads/master"), None);
        assert_eq!(StackedRef::parse("refs/remotes/origin/master"), None);
        assert_eq!(StackedRef::parse("master"), None);
        // A branch merely containing the substring is not a stacked ref.
        assert_eq!(StackedRef::parse("refs/heads/foo@changes-bar"), None);
        // Missing change-id or author.
        assert_eq!(
            StackedRef::parse("refs/heads/@changes/master/josh@example.com"),
            None
        );
        assert_eq!(StackedRef::parse("refs/heads/@changes/master/1234"), None);
        // A stack head must have nothing after the author.
        assert_eq!(
            StackedRef::parse("refs/heads/@heads/master/josh@example.com/extra"),
            None
        );
    }
}
