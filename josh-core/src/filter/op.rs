use super::Filter;
use crate::JoshResult;
use crate::josh_error;

#[derive(Hash, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum LazyRef {
    Resolved(git2::Oid),
    Lazy(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub fn parse(s: &str) -> JoshResult<LazyRef> {
        let s = s.replace("'", "\"");
        if let Ok(serde_json::Value::String(s)) = serde_json::from_str(&s) {
            return Ok(LazyRef::Lazy(s));
        }
        if let Ok(oid) = git2::Oid::from_str(&s) {
            Ok(LazyRef::Resolved(oid))
        } else {
            Err(josh_error(&format!("invalid ref: {:?}", s)))
        }
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Meta(std::collections::BTreeMap<String, String>, Filter),

    Nop,
    Empty,
    Fold,
    Paths,
    #[cfg(feature = "incubating")]
    Adapt(String),
    #[cfg(feature = "incubating")]
    Link(String),
    #[cfg(feature = "incubating")]
    Unlink,
    #[cfg(feature = "incubating")]
    Export,
    #[cfg(feature = "incubating")]
    Embed(std::path::PathBuf),

    // We use BTreeMap rather than HashMap to guarantee deterministic results when
    // converting to Filter
    Squash(Option<std::collections::BTreeMap<LazyRef, Filter>>),
    Author(String, String),
    Committer(String, String),

    // Vec instead of BTreeMap to preserve order - first match wins
    Rev(Vec<(RevMatch, LazyRef, Filter)>),
    Prune,
    RegexReplace(Vec<(regex::Regex, String)>),

    Hook(String),

    Index,
    Invert,

    File(std::path::PathBuf, std::path::PathBuf), // File(dest_path, source_path)
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),
    Stored(std::path::PathBuf),

    Pattern(String),
    Message(String, regex::Regex),

    #[cfg(feature = "incubating")]
    Unapply(LazyRef, Filter),

    Compose(Vec<Filter>),
    Chain(Vec<Filter>),
    Subtract(Filter, Filter),
    Exclude(Filter),
    Pin(Filter),
}
