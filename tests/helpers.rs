/* #![deny(warnings)] */
extern crate clap;
extern crate fern;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate grib;
extern crate hyper;
extern crate lazy_static;
extern crate log;
extern crate regex;
extern crate tempdir;
extern crate tokio_core;
use self::futures::Stream;
use self::futures::future::Future;
use self::hyper::header::{Authorization, Basic};
use self::hyper::server::Http;
use self::hyper::server::{Request, Response, Service};
use grib::*;
use grib::Shell;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use tempdir::TempDir;

pub struct TestRepo
{
    pub repo: git2::Repository,
    pub shell: Shell,
    td: TempDir,
}

impl TestRepo
{
    pub fn new() -> Self
    {
        let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
        let repo = git2::Repository::init(td.path()).expect("init should succeed");
        let tr = TestRepo {
            repo: repo,
            shell: Shell {
                cwd: td.path().to_path_buf(),
            },
            td: td,
        };
        tr.shell.command("git config user.name test");
        tr.shell.command("git config user.email test@test.com");
        return tr;
    }

    pub fn worktree(&self) -> &Path
    {
        self.td.path()
    }

    pub fn commit(&self, message: &str) -> String
    {
        self.shell
            .command(&format!("git commit -m \"{}\"", message));
        let (stdout, _) = self.shell.command("git rev-parse HEAD");
        stdout
    }

    pub fn add_file(&self, filename: &str)
    {
        self.shell
            .command(&format!("mkdir -p $(dirname {})", filename));
        self.shell
            .command(&format!("echo test_content > {}", filename));
        self.shell.command(&format!("git add {}", filename));
    }

    pub fn has_file(&self, filename: &str) -> bool
    {
        self.worktree().join(Path::new(filename)).exists()
    }

    pub fn rev(&self, r: &str) -> String
    {
        let (stdout, _) = self.shell.command(&format!("git rev-parse {}", r));
        stdout
    }
}
