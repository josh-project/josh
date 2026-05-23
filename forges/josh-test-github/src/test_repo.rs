use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use git2::Signature;

pub enum HookType {
    PostReceive,
}

pub struct TestRepo {
    dir: tempfile::TempDir,
    repo: git2::Repository,
    current_branch_ref: String,
}

pub const INITIAL_BRANCH_REF: &str = "refs/heads/main";

fn signature() -> Signature<'static> {
    Signature::new("test", "test@test.com", &git2::Time::new(0, 0)).unwrap()
}

impl TestRepo {
    pub fn new() -> anyhow::Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("josh-test-github")
            .tempdir()?;
        let repo = git2::Repository::init_bare(dir.path())?;
        repo.set_head(INITIAL_BRANCH_REF)?;
        Ok(Self {
            dir,
            repo,
            current_branch_ref: INITIAL_BRANCH_REF.to_string(),
        })
    }

    pub fn from_tempdir(tempdir: tempfile::TempDir) -> anyhow::Result<Self> {
        let repo = git2::Repository::open(tempdir.path())?;
        let current_branch_ref = repo
            .head()
            .ok()
            .and_then(|h| h.name().map(String::from))
            .unwrap_or_else(|| INITIAL_BRANCH_REF.to_string());
        Ok(Self {
            dir: tempdir,
            repo,
            current_branch_ref,
        })
    }

    pub fn install_hook(&mut self, _hook: HookType, contents: &str) -> anyhow::Result<()> {
        let hook_path = self.dir.path().join("hooks").join("post-receive");
        std::fs::write(&hook_path, contents)?;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    pub fn commit(
        &mut self,
        file_path: impl Into<PathBuf>,
        content: &str,
        message: Option<&str>,
    ) -> anyhow::Result<(git2::Oid, git2::Oid)> {
        let file_path: PathBuf = file_path.into();
        let sig = signature();
        let blob_oid = self.repo.blob(content.as_bytes())?;

        let parent = self.repo.revparse_single(&self.current_branch_ref).ok();
        let parent_commit = parent.as_ref().and_then(|obj| obj.as_commit());
        let parent_tree = parent_commit.map(|c| c.tree().unwrap());

        let mut treebuilder = self.repo.treebuilder(parent_tree.as_ref())?;
        treebuilder.insert(file_path, blob_oid, 0o100644)?;
        let tree_oid = treebuilder.write()?;
        let tree = self.repo.find_tree(tree_oid)?;

        let parents: Vec<&git2::Commit> = parent_commit.into_iter().collect();
        let commit_oid = self.repo.commit(
            Some(&self.current_branch_ref),
            &sig,
            &sig,
            message.unwrap_or("test commit"),
            &tree,
            &parents,
        )?;

        Ok((commit_oid, tree_oid))
    }

    pub fn select_create_branch(
        &mut self,
        branch_name: &str,
    ) -> anyhow::Result<Option<(git2::Oid, git2::Oid)>> {
        let branch_ref = format!("refs/heads/{}", branch_name);

        if self.repo.revparse_single(&branch_ref).is_ok() {
            self.current_branch_ref = branch_ref;
            return Ok(None);
        }

        let current_head = self.repo.revparse_single(&self.current_branch_ref)?.id();
        let current_commit = self.repo.find_commit(current_head)?;
        let tree = current_commit.tree()?;

        let sig = signature();
        let parents: [&git2::Commit; 1] = [&current_commit];
        let commit_oid = self.repo.commit(
            Some(&branch_ref),
            &sig,
            &sig,
            &format!("create branch {}", branch_name),
            &tree,
            &parents,
        )?;

        self.current_branch_ref = branch_ref;
        Ok(Some((commit_oid, tree.id())))
    }

    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().to_owned()
    }

    pub fn current_head(&self) -> anyhow::Result<git2::Oid> {
        let obj = self.repo.revparse_single(&self.current_branch_ref)?;
        Ok(obj.id())
    }

    pub fn current_branch_ref(&self) -> String {
        self.current_branch_ref.clone()
    }
}

impl AsRef<Path> for TestRepo {
    fn as_ref(&self) -> &Path {
        self.dir.path()
    }
}
