//! Views and links: named, versioned filter objects stored as git refs.
//!
//! A view is a filter plus pragmas, stored as flang text in a file versioned
//! under its own ref. A link is a view bound to an external remote: its `url`
//! and `tracked-ref` ride along as `:~(...)` meta options on the filter.

use anyhow::{Context, anyhow};
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::objects;
use std::path::Path;

/// The file inside a link ref's tree holding the view definition.
pub const LINK_FILE: &str = "link.josh";

/// Ref namespace links live in: `refs/josh/links/<id>`.
pub const LINKS_REF_PREFIX: &str = "refs/josh/links/";

const META_URL: &str = "url";
const META_TRACKED_REF: &str = "tracked-ref";

/// A view bound to an external remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    /// The main ref of the remote (e.g. `main` or `master`).
    pub tracked_ref: String,
}

/// A named, versioned binding of a filter plus optional link parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub filter: josh_filter::Filter,
    pub link: Option<Link>,
}

impl View {
    /// The view as a filter: the body wrapped in `:~(...)` meta carrying the
    /// link parameters when this view is a link.
    pub fn to_filter(&self) -> josh_filter::Filter {
        match &self.link {
            Some(link) => self
                .filter
                .with_meta(META_URL, link.url.clone())
                .with_meta(META_TRACKED_REF, link.tracked_ref.clone()),
            None => self.filter,
        }
    }

    /// Recover a view from a filter: link parameters come from the `url` and
    /// `tracked-ref` meta keys, which are stripped from the body; any other
    /// meta options stay on the filter.
    pub fn from_filter(filter: josh_filter::Filter) -> View {
        let link = match (filter.get_meta(META_URL), filter.get_meta(META_TRACKED_REF)) {
            (Some(url), Some(tracked_ref)) => Some(Link { url, tracked_ref }),
            _ => None,
        };
        View {
            filter: filter.without_meta_keys(&[META_URL, META_TRACKED_REF]),
            link,
        }
    }

    /// The `link.josh` file content for this view.
    pub fn to_file_string(&self) -> String {
        josh_filter::as_file(self.to_filter(), 0)
    }

    /// Parse a view from `link.josh` file content.
    pub fn parse(text: &str) -> anyhow::Result<View> {
        let filter = josh_core::filter::parse(text)
            .with_context(|| format!("failed to parse {LINK_FILE}"))?;
        Ok(View::from_filter(filter))
    }
}

/// The fully-qualified ref a link with `id` is versioned under.
pub fn link_ref_name(id: &str) -> String {
    format!("{LINKS_REF_PREFIX}{id}")
}

/// A link id becomes a git ref component, so reject anything that would make
/// the refname invalid or escape the links namespace.
pub fn validate_link_id(id: &str) -> anyhow::Result<()> {
    let invalid = id.is_empty()
        || id.starts_with('/')
        || id.ends_with('/')
        || id.ends_with('.')
        || id.contains("..")
        || id.contains("//")
        || id
            .chars()
            .any(|c| c.is_ascii_control() || " ~^:?*[\\".contains(c));
    if invalid {
        return Err(anyhow!("invalid link id '{id}'"));
    }
    Ok(())
}

/// Derive a link id from a remote URL: the last path segment (which for
/// scp-style URLs follows the host `:`), without the `.git` suffix.
pub fn derive_id_from_url(url: &str) -> Option<String> {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Commit `view` as `link.josh` onto the link's ref, with the previous tip as
/// parent when the ref already exists. The ref update is compare-and-swap
/// guarded against the tip read here, so a concurrent update is never
/// overwritten.
pub fn write_view(
    transaction: &Transaction,
    id: &str,
    view: &View,
    message: &str,
) -> anyhow::Result<gix_hash::ObjectId> {
    validate_link_id(id)?;
    if view.link.is_some() {
        for key in [META_URL, META_TRACKED_REF] {
            if view.filter.get_meta(key).is_some() {
                return Err(anyhow!(
                    "filter must not set reserved meta key '{key}': it is owned by the link config"
                ));
            }
        }
    }
    let refname = link_ref_name(id);
    let odb = transaction.odb();

    let blob = objects::write_blob(odb, view.to_file_string().as_bytes())?;

    let (base_tree, parents, guard) = if let Some(prev) = transaction.resolve_ref(&refname)? {
        let commit = objects::CommitData::read(odb, prev)?;
        (commit.tree_id()?, vec![prev], Expected::At(prev))
    } else {
        (tree::empty_id(), vec![], Expected::Absent)
    };

    let new_tree = tree::insert_oid(odb, base_tree, Path::new(LINK_FILE), blob, 0o0100644)?;
    let sig = josh_core::git::user_signature(transaction)?;
    let commit = objects::write_commit(odb, new_tree, &parents, &sig, &sig, message)?;
    transaction.update_ref(&refname, guard, commit, message)?;
    Ok(commit)
}

/// Read the view stored under the link's ref, or `None` when the link (or its
/// `link.josh` file) does not exist.
pub fn read_view(transaction: &Transaction, id: &str) -> anyhow::Result<Option<View>> {
    let Some(tip) = transaction.resolve_ref(&link_ref_name(id))? else {
        return Ok(None);
    };
    let odb = transaction.odb();
    let commit = objects::CommitData::read(odb, tip)?;
    let Some(entry) =
        tree::get_path_entry(transaction, odb, commit.tree_id()?, Path::new(LINK_FILE))?
    else {
        return Ok(None);
    };
    let Some(bytes) = tree::blob_bytes(odb, entry.oid) else {
        return Err(anyhow!("{LINK_FILE} in link '{id}' is not a blob"));
    };
    let text = std::str::from_utf8(&bytes)
        .with_context(|| format!("{LINK_FILE} in link '{id}' is not valid UTF-8"))?;
    View::parse(text)
        .with_context(|| format!("failed to load link '{id}'"))
        .map(Some)
}

/// Delete the link's ref. Errors when the link does not exist.
pub fn delete_view(transaction: &Transaction, id: &str) -> anyhow::Result<()> {
    let refname = link_ref_name(id);
    let Some(tip) = transaction.resolve_ref(&refname)? else {
        return Err(anyhow!("link '{id}' does not exist"));
    };
    transaction.delete_ref(&refname, Expected::At(tip))
}

/// All links in the repo, sorted by id.
pub fn list_views(transaction: &Transaction) -> anyhow::Result<Vec<(String, View)>> {
    let mut views = Vec::new();
    transaction.for_each_ref_prefixed(LINKS_REF_PREFIX, |name, _oid| {
        let id = name
            .strip_prefix(LINKS_REF_PREFIX)
            .unwrap_or(name)
            .to_string();
        if let Some(view) = read_view(transaction, &id)? {
            views.push((id, view));
        }
        Ok(())
    })?;
    Ok(views)
}

#[cfg(test)]
mod tests {
    use super::*;
    use josh_core::cache::{CacheStack, SledCacheBackend, TransactionContext};

    #[test]
    fn derive_id_from_url_cases() {
        assert_eq!(
            derive_id_from_url("https://example.com/repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_id_from_url("https://example.com/org/sub/repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_id_from_url("git@example.com:org/repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_id_from_url("ssh://git@example.com/repo"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_id_from_url("https://example.com/repo/"),
            Some("repo".to_string())
        );
        assert_eq!(derive_id_from_url(""), None);
    }

    #[test]
    fn view_file_round_trip() {
        let link_view = View {
            filter: josh_core::filter::parse(":/subfolder").unwrap(),
            link: Some(Link {
                url: "https://example.com/repo.git".to_string(),
                tracked_ref: "main".to_string(),
            }),
        };
        assert_eq!(View::parse(&link_view.to_file_string()).unwrap(), link_view);

        let plain_view = View {
            filter: josh_core::filter::parse(":/subfolder").unwrap(),
            link: None,
        };
        assert_eq!(
            View::parse(&plain_view.to_file_string()).unwrap(),
            plain_view
        );
    }

    #[test]
    fn parse_link_file_with_meta() {
        let view = View::parse(
            ":~(tracked-ref=\"main\",url=\"https://example.com/repo.git\")[:/subfolder]\n",
        )
        .unwrap();
        assert_eq!(
            view.link,
            Some(Link {
                url: "https://example.com/repo.git".to_string(),
                tracked_ref: "main".to_string(),
            })
        );
        assert_eq!(
            view.filter,
            josh_core::filter::parse(":/subfolder").unwrap()
        );
    }

    #[test]
    fn unrelated_meta_stays_on_filter() {
        let view = View::parse(
            ":~(history=\"linear\",tracked-ref=\"main\",url=\"https://example.com/repo.git\")[:/subfolder]\n",
        )
        .unwrap();
        assert!(view.link.is_some());
        assert_eq!(view.filter.get_meta("history"), Some("linear".to_string()));
    }

    fn open_transaction(td: &tempfile::TempDir) -> Transaction {
        gix::init_bare(td.path()).unwrap();
        // Commits need an identity; don't depend on the ambient global git
        // config (CI containers have none).
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .append(true)
            .open(td.path().join("config"))
            .unwrap()
            .write_all(b"\n[user]\n\tname = test\n\temail = test@example.com\n")
            .unwrap();

        let cachestack =
            std::sync::Arc::new(CacheStack::new().with_backend(SledCacheBackend::new(td.path())));
        TransactionContext::new(td.path(), cachestack)
            .open()
            .unwrap()
    }

    #[test]
    fn write_read_list_delete_round_trip() {
        let td = tempfile::tempdir().unwrap();
        let transaction = open_transaction(&td);

        let view = View {
            filter: josh_core::filter::parse(":/subfolder").unwrap(),
            link: Some(Link {
                url: "https://example.com/repo.git".to_string(),
                tracked_ref: "main".to_string(),
            }),
        };

        assert!(read_view(&transaction, "repo").unwrap().is_none());

        let first = write_view(&transaction, "repo", &view, "add link repo").unwrap();
        assert_eq!(read_view(&transaction, "repo").unwrap(), Some(view.clone()));

        // A second write keeps history: the new commit's parent is the first.
        let second = write_view(&transaction, "repo", &view, "update link repo").unwrap();
        let commit = objects::CommitData::read(transaction.odb(), second).unwrap();
        assert_eq!(commit.parent_ids().collect::<Vec<_>>(), vec![first]);

        assert_eq!(
            list_views(&transaction).unwrap(),
            vec![("repo".to_string(), view)]
        );

        delete_view(&transaction, "repo").unwrap();
        assert!(read_view(&transaction, "repo").unwrap().is_none());
        assert!(delete_view(&transaction, "repo").is_err());
    }

    #[test]
    fn write_rejects_reserved_meta_keys() {
        let td = tempfile::tempdir().unwrap();
        let transaction = open_transaction(&td);

        let view = View {
            filter: josh_core::filter::parse(
                ":~(url=\"https://example.com/other.git\")[:/subfolder]",
            )
            .unwrap(),
            link: Some(Link {
                url: "https://example.com/repo.git".to_string(),
                tracked_ref: "main".to_string(),
            }),
        };

        assert!(write_view(&transaction, "repo", &view, "add link repo").is_err());
        assert!(read_view(&transaction, "repo").unwrap().is_none());
    }
}
