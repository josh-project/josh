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



pub struct ServeTestGit
{
    handle: tokio_core::reactor::Handle,
    repo_path: PathBuf,
    username: String,
    password: String,
}

impl ServeTestGit {}


impl Service for ServeTestGit
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;


    fn call(&self, req: Request) -> Self::Future
    {
        let (username, password) = match req.headers().get() {
            Some(&Authorization(Basic {
                ref username,
                ref password,
            })) => (username.to_owned(), password.to_owned().unwrap_or("".to_owned()).to_owned()),
            _ => {
                println!("ServeTestGit: no credentials in request");
                let mut response = Response::new().with_status(hyper::StatusCode::Unauthorized);
                response
                    .headers_mut()
                    .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\"");
                return Box::new(futures::future::ok(response));
            }
        };

        if username != self.username || password != self.password {
            println!("ServeTestGit: wrong user/pass");
            let mut response = Response::new().with_status(hyper::StatusCode::Unauthorized);
            response
                .headers_mut()
                .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\"");
            return Box::new(futures::future::ok(response));
        }

        let path = &self.repo_path;

        let handle = self.handle.clone();

        println!("ServeTestGit CALLING git-http backend");
        let mut cmd = Command::new("git");
        cmd.arg("http-backend");
        cmd.current_dir(&path);
        cmd.env("GIT_PROJECT_ROOT", &path);
        cmd.env("GIT_DIR", &path);
        cmd.env("GIT_HTTP_EXPORT_ALL", "");
        cmd.env("PATH_INFO", req.uri().path());

        cgi::do_cgi(req, cmd, handle.clone())
    }
}

pub fn run_test_server(repo_path: &Path, port: u32)
{
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let server_handle = core.handle();



    let serve = loop
    {
        let repo_path = repo_path.to_owned();
        let h2 = core.handle();
        let make_service = move || {
            let cghttp = ServeTestGit {
                handle: h2.clone(),
                repo_path: repo_path.to_owned(),
                password: "".to_owned(),
                username: "".to_owned(),
            };
            Ok(cghttp)
        };
        let addr = format!("127.0.0.1:{}", port).parse().unwrap();
        if let Ok(serve) = Http::new()
            .serve_addr_handle(&addr, &server_handle, make_service)
        {
            break serve;
        }
    };


    let h2 = server_handle.clone();
    server_handle.spawn(
        serve
            .for_each(move |conn| {
                h2.spawn(
                    conn.map(|_| ())
                        .map_err(|err| println!("serve error:: {:?}", err)),
                );
                Ok(())
            })
            .map_err(|_| ()),
    );

    core.run(futures::future::empty::<(), ()>()).unwrap();
}
