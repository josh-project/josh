use allocative::Allocative;
use anyhow::Context;
use anyhow::anyhow;
use starlark::{
    environment::{MethodsBuilder, MethodsStatic},
    starlark_module, starlark_simple_value,
    values::{NoSerialize, ProvidesStaticType, StarlarkValue, StringValue, Value},
};
use std::fmt::{self, Display};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Opaque Tree type for Starlark
/// We wrap git2::Tree by storing its OID and a reference to the repository
#[derive(Clone, ProvidesStaticType, NoSerialize)]
pub struct StarlarkTree {
    pub tree_oid: git2::Oid,
    pub repo: Arc<Mutex<git2::Repository>>,
}

impl Allocative for StarlarkTree {
    fn visit<'a, 'b: 'a>(&self, _visitor: &'a mut allocative::Visitor<'b>) {
        // Tree OID is Copy and small, Repository is Arc so we don't need to visit it
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
        static RES: MethodsStatic = MethodsStatic::new();
        RES.methods(tree_methods)
    }
}

impl StarlarkTree {
    /// Create a new StarlarkTree from a git2::Oid
    pub fn new(tree_oid: git2::Oid, repo: Arc<Mutex<git2::Repository>>) -> Self {
        Self { tree_oid, repo }
    }

    /// Get empty tree OID
    fn empty_tree_oid() -> git2::Oid {
        git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap()
    }

    /// Navigate to a path in the tree, returning the OID of the tree at that path.
    /// Caller must hold repo lock when using navigate_to_path_oid_with_repo.
    fn navigate_to_path_oid_with_repo(
        &self,
        path: &str,
        repo: &git2::Repository,
    ) -> anyhow::Result<git2::Oid> {
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
            let current_tree = repo
                .find_tree(current_tree_oid)
                .context("Failed to find tree")?;

            let entry = current_tree
                .get_name(component)
                .ok_or_else(|| anyhow!("Path component '{}' not found", component))?;

            if entry.kind() != Some(git2::ObjectType::Tree) {
                return Err(anyhow!("Path component '{}' is not a directory", component));
            }

            current_tree_oid = entry.id();
        }

        Ok(current_tree_oid)
    }

    /// Navigate to a path in the tree, returning the OID of the tree at that path
    fn navigate_to_path_oid(&self, path: &str) -> anyhow::Result<git2::Oid> {
        let repo = self.repo.lock().unwrap();
        self.navigate_to_path_oid_with_repo(path, &repo)
    }

    /// Get blob content at path, returning empty string if not found or binary
    fn get_file_content(&self, path: &str) -> String {
        let repo = match self.repo.lock() {
            Ok(r) => r,
            Err(_) => return String::new(),
        };

        let tree = match repo.find_tree(self.tree_oid) {
            Ok(t) => t,
            Err(_) => return String::new(),
        };

        let entry = match tree.get_path(PathBuf::from(path).as_path()) {
            Ok(e) => e,
            Err(_) => return String::new(),
        };

        if entry.kind() != Some(git2::ObjectType::Blob) {
            return String::new();
        }

        let blob = match repo.find_blob(entry.id()) {
            Ok(b) => b,
            Err(_) => return String::new(),
        };

        if blob.is_binary() {
            return String::new();
        }

        std::str::from_utf8(blob.content())
            .map(|s| s.to_string())
            .unwrap_or_default()
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
        heap: &'v starlark::values::Heap,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        let repo = this.repo.lock().unwrap();

        let target_tree_oid = if path.as_str().is_empty() {
            this.tree_oid
        } else {
            match this.navigate_to_path_oid_with_repo(path.as_str(), &repo) {
                Ok(oid) => oid,
                Err(_) => return Ok(Vec::new()), // Path doesn't exist, return empty list
            }
        };

        let target_tree = repo
            .find_tree(target_tree_oid)
            .context("Failed to find tree")?;

        let mut dirs = Vec::new();
        let base_path = if path.as_str().is_empty() {
            String::new()
        } else {
            format!("{}/", path.as_str())
        };

        for entry in target_tree.iter() {
            if let Some(name) = entry.name() {
                if entry.kind() == Some(git2::ObjectType::Tree) {
                    let full_path = if base_path.is_empty() {
                        name.to_string()
                    } else {
                        format!("{}{}", base_path, name)
                    };
                    dirs.push(heap.alloc_str(&full_path).to_value());
                }
            }
        }

        Ok(dirs)
    }

    /// Get a list of full paths to child files (blobs) at the given path
    /// Returns empty list if the path doesn't exist
    fn files<'v>(
        this: &StarlarkTree,
        path: StringValue,
        heap: &'v starlark::values::Heap,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        let repo = this.repo.lock().unwrap();

        let target_tree_oid = if path.as_str().is_empty() {
            this.tree_oid
        } else {
            match this.navigate_to_path_oid_with_repo(path.as_str(), &repo) {
                Ok(oid) => oid,
                Err(_) => return Ok(Vec::new()), // Path doesn't exist, return empty list
            }
        };

        let target_tree = repo
            .find_tree(target_tree_oid)
            .context("Failed to find tree")?;

        let mut files = Vec::new();
        let base_path = if path.as_str().is_empty() {
            String::new()
        } else {
            format!("{}/", path.as_str())
        };

        for entry in target_tree.iter() {
            if let Some(name) = entry.name() {
                if entry.kind() == Some(git2::ObjectType::Blob) {
                    let full_path = if base_path.is_empty() {
                        name.to_string()
                    } else {
                        format!("{}{}", base_path, name)
                    };
                    files.push(heap.alloc_str(&full_path).to_value());
                }
            }
        }

        Ok(files)
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

            repo: this.repo.clone(),
        })
    }
}
