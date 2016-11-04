extern crate centralgithook;
extern crate git2;
extern crate tempdir;

use std::path::{Path, PathBuf};
use centralgithook::Shell;

pub struct TestRepo
{
    pub repo: git2::Repository,
    pub path: PathBuf,
    pub shell: Shell,
}

impl TestRepo
{
    pub fn new(path: &Path) -> Self
    {
        let tr = TestRepo {
            repo: git2::Repository::init(path).expect("init should succeed"),
            path: path.to_path_buf(),
            shell: Shell { cwd: path.to_path_buf() },
        };
        tr.shell.command("git config user.name test");
        tr.shell.command("git config user.email test@test.com");
        return tr;
    }

    pub fn commit(&self, message: &str) -> String
    {
        self.shell.command(&format!("git commit -m \"{}\"", message));
        let (stdout, _) = self.shell.command("git rev-parse HEAD");
        stdout
    }

    pub fn add_file(&self, filename: &str)
    {
        self.shell.command(&format!("mkdir -p $(dirname {})", filename));
        self.shell.command(&format!("echo test_content > {}", filename));
        self.shell.command(&format!("git add {}", filename));
    }

    pub fn has_file(&self, filename: &str) -> bool
    {
        self.path.join(Path::new(filename)).exists()
    }

    pub fn rev(&self, r: &str) -> String
    {
        let (stdout, _) = self.shell.command(&format!("git rev-parse {}", r));
        stdout
    }
}
