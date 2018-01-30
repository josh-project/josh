extern crate git2;

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use tempdir::TempDir;

pub struct Shell
{
    pub cwd: PathBuf,
}

impl Shell
{
    pub fn command(&self, cmd: &str) -> (String, String)
    {
        /* debug!("Shell::command: {}", cmd); */
        /* debug!("cwd: {:?}", self.cwd); */
        let git_dir = if self.cwd.join(".git").exists() {
            self.cwd.join(".git")
        }
        else {
            self.cwd.to_path_buf()
        };
        /* debug!("GIT_DIR: {:?}", git_dir); */

        let output = Command::new("sh")
            .current_dir(&self.cwd)
            .arg("-c")
            .arg(&cmd)
            .env("GIT_DIR", &git_dir)
            .output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}\n{}", cmd, e));

        let stdout =
            String::from_utf8(output.stdout).expect("failed to decode utf8").trim().to_string();
        let stderr =
            String::from_utf8(output.stderr).expect("failed to decode utf8").trim().to_string();
        /* debug!("stdout:\n{}", &stdout); */
        /* debug!("stderr:\n{}", &stderr); */
        /* debug!("\n"); */
        /* debug!("\n"); */
        return (stdout, stderr);
    }
}

struct TLocals { td: TempDir }

// This is just for debugging, to know when the TempDir actually gets removed
impl Drop for TLocals
{
    fn drop(&mut self)
    {
        /* println!("DROPPING {:?}", self.td.path()); */
        let shell = Shell { cwd: self.td.path().to_path_buf() };
        /* shell.command("git log HEAD"); */
        /* shell.command("ls -l"); */
        /* shell.command("ps -a"); */
        /* println!("done DROPPING {:?}", self.td.path()); */
    }
}

thread_local!(
    static TMP: RefCell<TLocals> = RefCell::new(
        TLocals { td: TempDir::new("centralgit").expect("failed to create tempdir") }
    )
);

pub fn thread_local_temp_dir() -> PathBuf
{
    let mut t = PathBuf::new();
    TMP.with(|tmp| {
        println!("old TMP {:?}", tmp.borrow().td.path());
        let x = TLocals { td: TempDir::new("centralgit").expect("failed to create tempdir") };
        t = x.td.path().to_path_buf();
        println!("creted TMP {:?}", t);
        *tmp.borrow_mut() = x;
    });
    t
}


