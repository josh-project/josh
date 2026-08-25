use allocative::Allocative;
use anyhow::Context;
use anyhow::anyhow;
use starlark::{
    environment::MethodsBuilder,
    starlark_module, starlark_simple_value,
    values::{NoSerialize, ProvidesStaticType, StarlarkValue, StringValue, Value},
};
use std::fmt::{self, Display};
use std::path::PathBuf;

/// Opaque Tree type for Starlark
/// We wrap a git tree by storing its OID and a raw pointer to the object source it came from.
#[derive(Clone, ProvidesStaticType, NoSerialize)]
pub(crate) struct StarlarkTree {
    pub tree_oid: gix_hash::ObjectId,
    // SAFETY: StarlarkTree is only constructed inside `evaluate()`, which is
    // synchronous and spawns no threads. The referenced object source must
    // remain alive and at a stable address for that entire duration so this raw
    // pointer stays valid.
    objects: *const dyn gix_object::Find,
}

// SAFETY: See the `objects` field documentation above.
unsafe impl Send for StarlarkTree {}
unsafe impl Sync for StarlarkTree {}

impl Allocative for StarlarkTree {
    fn visit<'a, 'b: 'a>(&self, _visitor: &'a mut allocative::Visitor<'b>) {
        // Tree OID is Copy and small; the object source is a raw pointer we do not own.
    }
}

starlark_simple_value!(StarlarkTree);

impl Display for StarlarkTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tree({})", self.tree_oid)
    }
}

impl fmt::Debug for StarlarkTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StarlarkTree(oid: {})", self.tree_oid)
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
    /// Create a new StarlarkTree from a tree OID and the object source to read it from.
    ///
    /// This constructor is crate-private because the returned value stores `objects`
    /// as a raw pointer without carrying a lifetime. The crate must therefore only
    /// construct `StarlarkTree` values in contexts that guarantee the object source
    /// outlives the tree and all of its clones, such as the synchronous
    /// `evaluate()` flow described in the struct-level safety comments.
    pub(crate) fn new(tree_oid: gix_hash::ObjectId, objects: &dyn gix_object::Find) -> Self {
        let objects: *const (dyn gix_object::Find + '_) = objects;
        Self {
            tree_oid,
            // SAFETY: erasing the borrow's lifetime is sound under the same contract that
            // makes the raw pointer sound -- the object source outlives this value and every
            // clone of it.
            objects: unsafe { std::mem::transmute(objects) },
        }
    }

    fn objects(&self) -> &dyn gix_object::Find {
        // SAFETY: See the `objects` field documentation on the struct.
        unsafe { &*self.objects }
    }

    /// Get empty tree OID
    fn empty_tree_oid() -> gix_hash::ObjectId {
        gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1)
    }

    /// Navigate to a path in the tree, returning the OID of the tree at that path
    fn navigate_to_path_oid(&self, path: &str) -> anyhow::Result<gix_hash::ObjectId> {
        if path.is_empty() {
            return Ok(self.tree_oid);
        }

        let path_buf = PathBuf::from(path);
        let components: Vec<&str> = path_buf
            .iter()
            .map(|c| c.to_str().context("Failed to convert path"))
            .collect::<Result<Vec<_>, _>>()?;

        let mut current_tree_oid = self.tree_oid;
        for component in components {
            let entry = josh_gix_ext::read_tree_entries(self.objects(), current_tree_oid)
                .context("Failed to find tree")?
                .into_iter()
                .find(|e| e.filename == component.as_bytes())
                .ok_or_else(|| anyhow!("Path component '{}' not found", component))?;

            if !entry.mode.is_tree() {
                return Err(anyhow!("Path component '{}' is not a directory", component));
            }

            current_tree_oid = entry.oid;
        }

        Ok(current_tree_oid)
    }

    /// Get blob content at path, returning empty string if not found or binary
    fn get_file_content(&self, path: &str) -> String {
        let objects = self.objects();
        let Ok(Some(entry)) =
            josh_gix_ext::path_entry(objects, self.tree_oid, PathBuf::from(path).as_path())
        else {
            return String::new();
        };
        if !entry.mode.is_blob() {
            return String::new();
        }
        josh_gix_ext::blob_text(objects, entry.oid)
    }

    /// The full paths of the entries at `path` that satisfy `keep`, in stored tree order.
    /// An unreadable path yields no entries, so a script can probe freely.
    fn child_paths(&self, path: &str, keep: fn(&gix_object::tree::Entry) -> bool) -> Vec<String> {
        let Ok(target_tree_oid) = self.navigate_to_path_oid(path) else {
            return Vec::new();
        };
        let Ok(entries) = josh_gix_ext::read_tree_entries(self.objects(), target_tree_oid) else {
            return Vec::new();
        };
        let base_path = if path.is_empty() {
            String::new()
        } else {
            format!("{}/", path)
        };
        entries
            .iter()
            .filter(|entry| keep(entry))
            .filter_map(|entry| std::str::from_utf8(&entry.filename).ok())
            .map(|name| format!("{}{}", base_path, name))
            .collect()
    }
}

#[starlark_module]
fn tree_methods(_builder: &mut MethodsBuilder) {
    /// Get the content of a file at the given path
    /// Returns empty string if the file doesn't exist or is binary
    fn file(this: &StarlarkTree, path: StringValue) -> anyhow::Result<String> {
        Ok(this.get_file_content(path.as_str()))
    }

    /// Get a list of full paths to child directories at the given path
    /// Returns empty list if the path doesn't exist
    fn dirs<'v>(
        this: &StarlarkTree,
        path: StringValue,
        heap: starlark::values::Heap<'v>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        Ok(this
            .child_paths(path.as_str(), |entry| entry.mode.is_tree())
            .iter()
            .map(|path| heap.alloc_str(path).to_value())
            .collect())
    }

    /// Get a list of full paths to child files (blobs) at the given path
    /// Returns empty list if the path doesn't exist
    fn files<'v>(
        this: &StarlarkTree,
        path: StringValue,
        heap: starlark::values::Heap<'v>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        Ok(this
            .child_paths(path.as_str(), |entry| entry.mode.is_blob())
            .iter()
            .map(|path| heap.alloc_str(path).to_value())
            .collect())
    }

    /// Get the tree at the given path
    /// Returns an empty tree if the path doesn't exist
    fn tree(this: &StarlarkTree, path: StringValue) -> anyhow::Result<StarlarkTree> {
        let tree_oid = match this.navigate_to_path_oid(path.as_str()) {
            Ok(oid) => oid,
            Err(_) => StarlarkTree::empty_tree_oid(), // Path doesn't exist, return empty tree
        };

        Ok(StarlarkTree {
            tree_oid,
            objects: this.objects,
        })
    }
}
