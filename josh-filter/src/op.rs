use crate::filter::Filter;
use anyhow::anyhow;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkMode {
    Embedded,
    Snapshot,
    Pointer,
}

/// Newtype around `regex::Regex` adding structural `PartialEq`/`Eq`/`Hash` (by pattern
/// string) so `Op` can derive them for use as an interning key. Derefs to the inner regex.
#[derive(Clone, Debug)]
pub struct Regex(pub regex::Regex);

impl std::ops::Deref for Regex {
    type Target = regex::Regex;
    fn deref(&self) -> &regex::Regex {
        &self.0
    }
}

impl PartialEq for Regex {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for Regex {}

impl std::hash::Hash for Regex {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state);
    }
}

impl std::fmt::Display for LinkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkMode::Embedded => write!(f, "embedded"),
            LinkMode::Snapshot => write!(f, "snapshot"),
            LinkMode::Pointer => write!(f, "pointer"),
        }
    }
}

impl LinkMode {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "embedded" => Ok(LinkMode::Embedded),
            "snapshot" => Ok(LinkMode::Snapshot),
            "pointer" => Ok(LinkMode::Pointer),
            _ => Err(anyhow!("Unknown link mode: {:?}", s)),
        }
    }
}

#[derive(Hash, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum LazyRef {
    Resolved(git2::Oid),
    Lazy(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InsertContent {
    Inline(String),
    /// An object referenced by OID. The kind (blob or tree) is resolved against a repository
    /// when the filter is applied or persisted; `persist::as_tree` references the object as a
    /// tree entry with the matching mode so it is reachable from the filter tree.
    Oid(git2::Oid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RevMatch {
    /// `<` - matches if is_ancestor_of(commit, tip) && commit != tip (strict)
    AncestorStrict,
    /// `<=` - matches if is_ancestor_of(commit, tip) || commit == tip (inclusive)
    AncestorInclusive,
    /// `==` - matches if commit == tip
    Equal,
    /// `_` - default filter when no other matches (no SHA needed)
    Default,
}

impl std::fmt::Display for LazyRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LazyRef::Resolved(id) => write!(f, "{}", id),
            LazyRef::Lazy(lazy) => write!(f, "\"{}\"", lazy),
        }
    }
}

impl LazyRef {
    pub fn parse(s: &str) -> anyhow::Result<LazyRef> {
        let s = s.replace("'", "\"");
        if let Ok(serde_json::Value::String(s)) = serde_json::from_str(&s) {
            return Ok(LazyRef::Lazy(s));
        }

        if let Ok(oid) = git2::Oid::from_str(&s) {
            Ok(LazyRef::Resolved(oid))
        } else {
            Err(anyhow!("invalid ref: {:?}", s))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Meta(std::collections::BTreeMap<String, String>, Filter),

    Nop,
    Empty,
    Fold,
    Paths,
    Adapt(String),
    Link(Option<LinkMode>),
    Unlink,
    Export,
    Embed(std::path::PathBuf),

    // We use BTreeMap rather than HashMap to guarantee deterministic results when
    // converting to Filter
    Squash(Option<std::collections::BTreeMap<LazyRef, Filter>>),
    Author(String, String),
    Committer(String, String),

    // Vec instead of BTreeMap to preserve order - first match wins
    Rev(Vec<(RevMatch, LazyRef, Filter)>),
    Prune,
    RegexReplace(Vec<(Regex, String)>),

    Hook(String),

    Index,
    Invert,

    Insert(std::path::PathBuf, InsertContent), // Insert(dest_path, content)
    File(std::path::PathBuf, std::path::PathBuf), // File(dest_path, source_path)
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),
    Stored(std::path::PathBuf),
    Starlark(std::path::PathBuf, Filter),
    TreeId(std::path::PathBuf, Filter),
    ObjectDeref(std::path::PathBuf),
    ObjectRef(std::path::PathBuf),

    Pattern(String),
    Message(String, Regex),

    Unapply(LazyRef, Filter),

    Compose(Vec<Filter>),
    Chain(Vec<Filter>),
    Subtract(Filter, Filter),
    Exclude(Filter),
    Select(Filter),
    Pin(Filter),

    Downstack(LazyRef),
}
