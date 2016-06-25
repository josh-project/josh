extern crate centralgithook;
extern crate git2;
extern crate tempdir;

use centralgithook::migrate;
use std::fs::File;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempdir::TempDir;

pub fn _oid_to_sha1(oid: &[u8]) -> String
{
    oid.iter()
        .fold(Vec::new(), |mut acc, x| {
            acc.push(format!("{0:>02x}", x));
            acc
        })
        .concat()
}

pub struct TestHost
{
    td: TempDir,
}

impl TestHost
{
    pub fn new() -> Self
    {
        TestHost { td: TempDir::new("test_host").expect("folder test_host should be created") }
    }
}

impl migrate::RepoHost for TestHost
{
    fn create_project(&self, module: &str) -> Result<(), git2::Error>
    {
        let repo_dir = self.td.path().join(&Path::new(module));
        println!("TestHost: create_project {} in {:?}",
                 module,
                 repo_dir);
        git2::Repository::init_bare(&repo_dir).expect("TestHost: init_bare failed");
        // empty_commit(&repo);
        Ok(())
    }

    fn remote_url(&self, module_path: &str) -> String
    {
        self.td.path().join(&module_path).to_string_lossy().to_string()
    }
}

pub struct TestRepo
{
    repo: git2::Repository,
    pub path: PathBuf,
    pub shell: migrate::Shell,
}

impl TestRepo
{
    pub fn new(path: &Path) -> Self
    {
        TestRepo {
            repo: git2::Repository::init(path).expect("init should succeed"),
            path: path.to_path_buf(),
            shell: migrate::Shell { cwd: path.to_path_buf() },
        }
    }

    pub fn commit(&self, message: &str) -> String
    {
        self.shell.command(&format!("git commit -m \"{}\"", message));
        return _oid_to_sha1(self.repo.revparse_single("HEAD").expect("no HEAD").id().as_bytes());
    }

    pub fn add(&self, filename: &str)
    {
        let f = self.path.join(filename);
        let parent_dir = f.parent().expect("need to get parent");
        fs::create_dir_all(parent_dir).expect("create directories");

        let mut file = File::create(&f).expect("create file");
        file.write_all("test content".as_bytes()).expect("write to file");
        self.shell.command(&format!("git add {}", filename));
    }
}



// fn empty_commit(repo: &git2::Repository) {
//     let sig = git2::Signature::now("foo", "bar").expect("created signature");
//     repo.commit(
//         Some("HEAD"),
//         &sig,
//         &sig,
//         "initial",
//         &repo.find_tree(repo.treebuilder(None).expect("cannot create empty tree")
// .write().expect("cannot write empty tree")).expect("cannot find empty
// tree"),
//         &[]
//     ).expect("cannot commit empty");
// }
