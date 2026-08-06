//! The script-facing `tree.*` surface, mirroring the old native
//! `josh-starlark/src/tree.rs` — same collapse-to-empty semantics, but no
//! unsafe repo pointer: all access goes through the host tree imports, which
//! address entries by full path from the context root. A sub-tree is
//! therefore nothing but a path prefix.

use allocative::Allocative;
use starlark::{
    environment::MethodsBuilder,
    starlark_module, starlark_simple_value,
    values::{NoSerialize, ProvidesStaticType, StarlarkValue, StringValue, Value},
};
use std::fmt::{self, Display};

/// OID of the empty tree, printed for sub-trees at nonexistent paths to match
/// the old native repr.
const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Opaque Tree type for Starlark
#[derive(Clone, ProvidesStaticType, NoSerialize)]
pub(crate) struct StarlarkTree {
    /// Path of this tree relative to the context root; empty for the root.
    prefix: String,
}

impl Allocative for StarlarkTree {
    fn visit<'a, 'b: 'a>(&self, _visitor: &'a mut allocative::Visitor<'b>) {
        // Only a path prefix; nothing worth visiting.
    }
}

starlark_simple_value!(StarlarkTree);

impl Display for StarlarkTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let oid = josh_filter_guest::Tree::root()
            .entry_oid(&self.prefix)
            .unwrap_or_else(|| String::from(EMPTY_TREE_OID));
        write!(f, "Tree({})", oid)
    }
}

impl fmt::Debug for StarlarkTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StarlarkTree(prefix: {:?})", self.prefix)
    }
}

impl<'v> StarlarkValue<'v> for StarlarkTree {
    type Canonical = Self;

    const TYPE: &'static str = "Tree";

    fn get_type_starlark_repr() -> starlark::typing::Ty {
        starlark::typing::Ty::starlark_value::<Self>()
    }

    fn get_methods() -> Option<&'static starlark::environment::Methods> {
        starlark::methods_static!(RES = tree_methods);
        Some(RES.methods())
    }
}

impl StarlarkTree {
    /// The root of the context-filtered tree.
    pub(crate) fn root() -> Self {
        Self {
            prefix: String::new(),
        }
    }

    /// Full path from the context root for a script-supplied path.
    fn join(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            String::from(path)
        } else if path.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{}", self.prefix, path)
        }
    }

    /// The host returns full paths from the context root; the script expects
    /// paths relative to this tree (i.e. starting with the path it passed).
    fn strip_prefix(&self, full: Vec<String>) -> Vec<String> {
        if self.prefix.is_empty() {
            return full;
        }
        let prefix = format!("{}/", self.prefix);
        full.into_iter()
            .map(|p| match p.strip_prefix(&prefix) {
                Some(rel) => String::from(rel),
                None => p,
            })
            .collect()
    }
}

#[starlark_module]
fn tree_methods(_builder: &mut MethodsBuilder) {
    /// Get the content of a file at the given path
    /// Returns empty string if the file doesn't exist or is binary
    fn file(this: &StarlarkTree, path: StringValue) -> anyhow::Result<String> {
        Ok(josh_filter_guest::Tree::root().file(&this.join(path.as_str())))
    }

    /// Get a list of full paths to child directories at the given path
    /// Returns empty list if the path doesn't exist
    fn dirs<'v>(
        this: &StarlarkTree,
        path: StringValue,
        heap: starlark::values::Heap<'v>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        let dirs = josh_filter_guest::Tree::root().dirs(&this.join(path.as_str()));
        Ok(this
            .strip_prefix(dirs)
            .into_iter()
            .map(|p| heap.alloc_str(&p).to_value())
            .collect())
    }

    /// Get a list of full paths to child files (blobs) at the given path
    /// Returns empty list if the path doesn't exist
    fn files<'v>(
        this: &StarlarkTree,
        path: StringValue,
        heap: starlark::values::Heap<'v>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        let files = josh_filter_guest::Tree::root().files(&this.join(path.as_str()));
        Ok(this
            .strip_prefix(files)
            .into_iter()
            .map(|p| heap.alloc_str(&p).to_value())
            .collect())
    }

    /// Get the tree at the given path
    /// Returns an empty tree if the path doesn't exist
    fn tree(this: &StarlarkTree, path: StringValue) -> anyhow::Result<StarlarkTree> {
        Ok(StarlarkTree {
            prefix: this.join(path.as_str()),
        })
    }
}
