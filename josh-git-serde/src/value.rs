use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub enum GitValue {
    Blob(Vec<u8>),
    Tree(BTreeMap<String, Box<GitValue>>),
}

impl fmt::Debug for GitValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitValue::Blob(data) => {
                const MAX: usize = 64;
                // Truncated so a {:?} on a huge blob cannot flood a log or
                // an assert_eq! failure message.
                match std::str::from_utf8(data) {
                    Ok(s) if data.len() <= MAX => {
                        formatter.write_fmt(format_args!("Blob {{ {s:?} }}"))
                    }
                    _ => formatter.write_fmt(format_args!("Blob {{ <{} bytes> }}", data.len())),
                }
            }
            GitValue::Tree(entries) => {
                write!(formatter, "Tree ")?;
                formatter.debug_map().entries(entries.iter()).finish()
            }
        }
    }
}

impl GitValue {
    pub fn empty_blob() -> Self {
        GitValue::Blob(Vec::new())
    }

    pub fn blob_from_str(s: impl AsRef<[u8]>) -> Self {
        GitValue::Blob(s.as_ref().to_vec())
    }
}
