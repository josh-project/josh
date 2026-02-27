use crate::op::{LazyRef, Op};
use crate::opt;
use crate::persist::{self, to_filter, to_op};
use std::sync::LazyLock;

/// Match-all regex pattern used as the default for Op::Message when no regex is specified.
/// The pattern `(?s)^.*$` matches any string (including newlines) from start to end.
pub static MESSAGE_MATCH_ALL_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("(?s)^.*$").unwrap());

/// Filters are represented as `git2::Oid`, however they are not ever stored
/// inside the repo.
#[derive(Clone, Hash, PartialEq, Eq, Copy, PartialOrd, Ord)]
pub struct Filter(pub git2::Oid);

impl Default for Filter {
    fn default() -> Filter {
        Filter::new()
    }
}

impl Filter {
    pub fn id(&self) -> git2::Oid {
        self.0
    }

    /// Create a Filter from an Oid. This is primarily used for special filters
    /// like sequence_number that don't correspond to a normal Op variant.
    pub(crate) fn from_oid(oid: git2::Oid) -> Filter {
        Filter(oid)
    }
}

impl Filter {
    /// Create a no-op filter that passes everything through unchanged
    pub fn new() -> Filter {
        to_filter(Op::Nop)
    }

    /// Create a filter that is the result of feeding the output of `first` into `second`
    pub fn chain(self, second: Filter) -> Filter {
        opt::optimize(to_filter(Op::Chain(vec![self, second])))
    }

    /// Create a no-op filter that passes everything through unchanged
    pub fn nop(self) -> Filter {
        self
    }

    pub fn is_nop(self) -> bool {
        self == to_filter(Op::Nop)
    }

    /// Create a filter that produces an empty tree
    pub fn empty(self) -> Filter {
        to_filter(Op::Empty)
    }

    /// Chain a filter that ensures linear history by dropping all parents
    /// of commits except the first parent
    pub fn linear(self) -> Filter {
        self.with_meta("history", "linear")
    }

    /// Chain a file filter that selects a single file
    pub fn file(self, path: impl Into<std::path::PathBuf>) -> Filter {
        let p = path.into();
        self.rename(p.clone(), p)
    }

    /// Chain a filter that renames a file from `src` to `dst`
    /// The file is extracted from the source path and placed at the destination path
    pub fn rename(
        self,
        dst: impl Into<std::path::PathBuf>,
        src: impl Into<std::path::PathBuf>,
    ) -> Filter {
        self.chain(to_filter(Op::File(dst.into(), src.into())))
    }

    /// Chain a filter that selects a subdirectory from the tree
    /// Only the contents of the specified directory are included
    pub fn subdir(self, path: impl Into<std::path::PathBuf>) -> Filter {
        self.chain(to_filter(Op::Subdir(path.into())))
    }

    /// Chain a filter that adds a prefix path to the tree
    /// The entire tree is placed under the specified directory path
    pub fn prefix(self, path: impl Into<std::path::PathBuf>) -> Filter {
        self.chain(to_filter(Op::Prefix(path.into())))
    }

    /// Chain a filter that loads a stored filter from a file
    /// The filter is read from a `.josh` file at the specified path
    pub fn stored(self, path: impl Into<std::path::PathBuf>) -> Filter {
        self.chain(to_filter(Op::Stored(path.into())))
    }

    /// Chain a filter that evaluates a Starlark script from a `.star` file.
    /// The path is used with `.star` extension. The subfilter is applied to the input tree to get the tree passed to the script.
    /// Syntax: `:*starfile[:filter]` (e.g. `:*foo[:/lib]` passes the result of `:/lib` to the script).
    #[cfg(feature = "incubating")]
    pub fn starlark(self, path: impl Into<std::path::PathBuf>, subfilter: Filter) -> Filter {
        self.chain(to_filter(Op::Starlark(path.into(), subfilter)))
    }

    /// Chain a filter that removes the `.link.josh` marker to produce a standalone history
    #[cfg(feature = "incubating")]
    pub fn export(self) -> Filter {
        self.chain(to_filter(Op::Export))
    }

    /// Chain a filter that matches files by glob pattern
    /// Only files matching the pattern are included in the result
    pub fn pattern(self, p: impl Into<String>) -> Filter {
        self.chain(to_filter(Op::Pattern(p.into())))
    }

    /// Chain a filter that loads a workspace filter from a `workspace.josh` file
    /// The workspace filter is read from the specified directory path
    pub fn workspace(self, path: impl Into<std::path::PathBuf>) -> Filter {
        self.chain(to_filter(Op::Workspace(path.into())))
    }

    /// Chain a filter that sets the author name and email for commits
    pub fn author(self, name: impl Into<String>, email: impl Into<String>) -> Filter {
        self.chain(to_filter(Op::Author(name.into(), email.into())))
    }

    /// Chain a filter that sets the committer name and email for commits
    pub fn committer(self, name: impl Into<String>, email: impl Into<String>) -> Filter {
        self.chain(to_filter(Op::Committer(name.into(), email.into())))
    }

    /// Chain a filter that prunes trivial merge commits
    /// Removes merge commits where the tree is identical to the first parent
    pub fn prune_trivial_merge(self) -> Filter {
        self.chain(to_filter(Op::Prune))
    }

    /// Chain a filter that removes commit signatures
    /// The filtered commits will not have GPG signatures
    pub fn unsign(self) -> Filter {
        self.with_meta("signature", "remove")
    }

    /// Chain a squash filter
    pub fn squash(self, ids: Option<&[(git2::Oid, Filter)]>) -> Filter {
        self.chain(if let Some(ids) = ids {
            to_filter(Op::Squash(Some(
                ids.iter()
                    .map(|(x, y)| (LazyRef::Resolved(*x), *y))
                    .collect(),
            )))
        } else {
            to_filter(Op::Squash(None))
        })
    }

    /// Chain a message filter that transforms commit messages
    pub fn message(self, m: &str) -> Filter {
        self.chain(to_filter(Op::Message(
            m.to_string(),
            MESSAGE_MATCH_ALL_REGEX.clone(),
        )))
    }

    /// Chain a message filter that transforms commit messages
    pub fn message_regex(self, m: impl Into<String>, regex: regex::Regex) -> Filter {
        self.chain(to_filter(Op::Message(m.into(), regex)))
    }

    /// Chain a hook filter
    pub fn hook(self, h: &str) -> Filter {
        self.chain(to_filter(Op::Hook(h.to_string())))
    }

    /// Wrap this filter with metadata (a single key-value pair)
    /// The metadata is stored alongside the filter
    /// If the filter is already wrapped in Meta, the new metadata entry is merged with existing ones
    /// (new entries take precedence over existing ones with the same key)
    pub fn with_meta<K, V>(self, key: K, value: V) -> Filter
    where
        K: Into<String>,
        V: Into<String>,
    {
        let key = key.into();
        let value = value.into();
        let op = to_op(self);
        match op {
            Op::Meta(mut existing_meta, inner_filter) => {
                // Merge existing metadata with new metadata (new entries take precedence)
                existing_meta.insert(key, value);
                to_filter(Op::Meta(existing_meta, inner_filter))
            }
            _ => {
                // Filter doesn't have metadata, wrap it
                let mut new_meta = std::collections::BTreeMap::new();
                new_meta.insert(key, value);
                to_filter(Op::Meta(new_meta, self))
            }
        }
    }

    /// Get a metadata value by key from this filter
    /// Returns None if the filter doesn't have metadata or the key doesn't exist
    pub fn get_meta(&self, key: &str) -> Option<String> {
        let op = to_op(*self);
        match op {
            Op::Meta(meta, _) => meta.get(key).cloned(),
            _ => None,
        }
    }

    /// Get all metadata from this filter as a BTreeMap
    /// Returns an empty BTreeMap if the filter doesn't have metadata
    pub fn into_meta(self) -> std::collections::BTreeMap<String, String> {
        let op = to_op(self);
        match op {
            Op::Meta(meta, _) => meta,
            _ => std::collections::BTreeMap::new(),
        }
    }

    /// Peel away metadata layers to get the inner filter
    /// Recursively removes all Meta wrappers until reaching the actual filter
    /// If the filter doesn't have metadata, returns the filter itself
    pub fn peel(&self) -> Filter {
        let op = to_op(*self);
        match op {
            Op::Meta(_, inner_filter) => inner_filter.peel(),
            _ => *self,
        }
    }
}

impl std::fmt::Debug for Filter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        to_op(*self).fmt(f)
    }
}

/// Create a filter that is the result of overlaying the output of filters in a vector
/// sequentially; so f(0) -> f(1) -> ... -> f(N)
pub fn compose(filters: &[Filter]) -> Filter {
    opt::optimize(persist::to_filter(Op::Compose(filters.to_vec())))
}

pub fn invert(filter: Filter) -> anyhow::Result<Filter> {
    opt::invert(filter)
}

/// Create a sequence_number filter used for tracking commit sequence numbers
pub fn sequence_number() -> Filter {
    Filter::from_oid(git2::Oid::zero())
}
