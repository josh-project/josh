use super::Filter;
use crate::JoshResult;
use crate::josh_error;

#[derive(Hash, Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum LazyRef {
    Resolved(git2::Oid),
    Lazy(String),
}

impl LazyRef {
    pub fn to_string(&self) -> String {
        match self {
            LazyRef::Resolved(id) => format!("{}", id),
            LazyRef::Lazy(lazy) => format!("\"{}\"", lazy),
        }
    }
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

    // We use BTreeMap rather than HashMap to guarantee deterministic results when
    // converting to Filter
    Rev(std::collections::BTreeMap<LazyRef, Filter>),
    Linear,
    Prune,
    Unsign,

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

    HistoryConcat(LazyRef, Filter),
    #[cfg(feature = "incubating")]
    Unapply(LazyRef, Filter),

    Compose(Vec<Filter>),
    Chain(Vec<Filter>),
    Subtract(Filter, Filter),
    Exclude(Filter),
    Pin(Filter),
}
