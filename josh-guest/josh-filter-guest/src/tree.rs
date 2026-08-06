//! Read-only access to the context-filtered tree.

use crate::abi;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// A position inside the context-filtered tree.
///
/// The host imports always address entries by their full path from the
/// context root, so a sub-`Tree` is nothing but a path prefix that gets
/// prepended to every query. There is no way to navigate outside the
/// context-filtered tree.
#[derive(Clone, Debug)]
pub struct Tree {
    prefix: String,
}

impl Tree {
    /// The root of the context-filtered tree.
    pub fn root() -> Tree {
        Tree {
            prefix: String::new(),
        }
    }

    fn join(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            String::from(path)
        } else if path.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{}", self.prefix, path)
        }
    }

    /// Content of the blob at `path`; the empty string for absent, non-blob,
    /// binary or non-UTF-8 entries (indistinguishable from an empty file —
    /// use [`Tree::entry_oid`] to test for existence).
    pub fn file(&self, path: &str) -> String {
        let path = self.join(path);
        crate::decode_string(unsafe { abi::tree_file(path.as_ptr(), path.len() as u32) })
    }

    /// Full slash-joined paths (from the context root, regardless of this
    /// tree's prefix) of the immediate child blobs of the tree at `path`, in
    /// git tree entry order. A bad path yields an empty list.
    pub fn files(&self, path: &str) -> Vec<String> {
        let path = self.join(path);
        crate::split_list(crate::decode_string(unsafe {
            abi::tree_files(path.as_ptr(), path.len() as u32)
        }))
    }

    /// Like [`Tree::files`], but the immediate child trees. Submodule
    /// entries appear in neither.
    pub fn dirs(&self, path: &str) -> Vec<String> {
        let path = self.join(path);
        crate::split_list(crate::decode_string(unsafe {
            abi::tree_dirs(path.as_ptr(), path.len() as u32)
        }))
    }

    /// Hex OID of the entry at `path`; `None` if absent. The empty path
    /// yields the OID of this tree itself.
    pub fn entry_oid(&self, path: &str) -> Option<String> {
        let path = self.join(path);
        let oid =
            crate::decode_string(unsafe { abi::tree_entry_oid(path.as_ptr(), path.len() as u32) });
        if oid.is_empty() { None } else { Some(oid) }
    }

    /// Navigate to the sub-tree at `path` (pure path arithmetic; the path
    /// need not exist — queries on a nonexistent tree yield empty results).
    pub fn tree(&self, path: &str) -> Tree {
        Tree {
            prefix: self.join(path),
        }
    }
}
