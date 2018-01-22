extern crate centralgithook;
extern crate fern;
extern crate git2;
extern crate regex;
extern crate tempdir;

#[macro_use]
extern crate log;

use centralgithook::*;
use git2::Oid;
use regex::Regex;
use std::env::current_exe;
use std::env;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
//use std::process::Stdio;
use std::process::exit;
use tempdir::TempDir;

use std::cell::RefCell;
use std::thread;

struct TLocals
{
    td: TempDir,
}

impl Drop for TLocals
{
    fn drop(&mut self)
    {
        println!("DROPPING {:?}", self.td.path());
        let shell = Shell { cwd: self.td.path().to_path_buf() };
        shell.command("git log HEAD");
        shell.command("ls -l");
        println!("done DROPPING {:?}", self.td.path());
    }
}

thread_local!(
    static TMP: RefCell<TLocals> = RefCell::new(
        TLocals {
            td: TempDir::new("centralgit").expect("failed to create tempdir")
        }
    )
);



// #[macro_use]
extern crate rouille;

use std::io;
use rouille::cgi::CgiRun;

fn setup_tmp_repo(scratch_dir: &Path, view: Option<&str>) -> PathBuf
{
    let path = {
        let mut t = PathBuf::new();
        TMP.with(|tmp| {
            println!("old TMP {:?}", tmp.borrow().td.path());
            let x = TLocals { td: TempDir::new("centralgit").expect("failed to create tempdir") };
            t = x.td.path().to_path_buf();
            println!("creted TMP {:?}", t);
            *tmp.borrow_mut() = x;
        });
        t
    };

    let root = match view {
        Some(view) => view_ref_root(&view),
        None => "refs".to_string(),
    };

    debug!("setup_tmp_repo, root: {:?}", &root);
    let shell = Shell { cwd: path.to_path_buf() };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, path.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), path));
    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("config"), path));
    symlink(scratch_dir.join(root), path.join("refs")).expect("can't symlink refs");
    symlink(scratch_dir.join("objects"), path.join("objects")).expect("can't symlink objects");

    shell.command(&format!("printf {} > view",
                           match view {
                               Some(view) => view,
                               None => ".",
                           }));

    shell.command(&format!("printf {} > orig", scratch_dir.to_string_lossy()));
    /* shell.command("ls"); */
    return path;
}

fn main() { exit(main_ret()); }

fn main_ret() -> i32 {

    let mut args = vec![];
    for arg in env::args() {
        args.push(arg);
    }
    debug!("args: {:?}", args);

    let logfilename = Path::new("/tmp/centralgit.log");
    fern::Dispatch::new()
    .format(|out, message, record| {
        out.finish(format_args!(
            "{}[{}] {}",
            record.target(),
            record.level(),
            message
        ))
    })
    .level(log::LevelFilter::Debug)
    .chain(std::io::stdout())
    .chain(fern::log_file(logfilename).unwrap())
    .apply().unwrap();

    if args[0].ends_with("/update") {
        debug!("================= HOOK {:?}", args);
        return update_hook(&args[1], &args[2], &args[3]);
    }

    println!("Now listening on localhost:8000");


    rouille::start_server("localhost:8000", move |request| {
        rouille::log(&request, io::stdout(), || {


            println!("X\nX\nX\nURL: {}", request.url());
            let re = Regex::new(r"(?P<prefix>/.*[.]git)/.*").expect("can't compile regex");

            let prefix = if let Some(caps) = re.captures(&request.url()) {
                caps.name("prefix").expect("can't find name prefix").as_str().to_string()
            }
            else {
                String::new()
            };


            let scratch = Scratch::new(&env::current_dir().unwrap());

            let re = Regex::new(r"/(?P<view>.*)[.]git/.*").expect("can't compile regex");

            let view_repo = if let Some(caps) = re.captures(&request.url()) {
                let view = caps.name("view").unwrap();
                println!("VIEW {}", view.as_str());

                let view = caps.name("view").unwrap();

                for branch in scratch.repo.branches(None).expect("could not get branches") {
                    if let Ok((branch, _)) = branch {
                        let branchname = branch.name().unwrap().unwrap().to_string();
                        let r = branch.into_reference().target().expect("no ref");
                        let viewobj = SubdirView::new(&Path::new(&view.as_str()));
                        let view_commit = scratch.apply_view(&viewobj, r).expect("can't apply view");
                        scratch.repo
                            .reference(&view_ref(&view.as_str(), &branchname),
                                       view_commit,
                                       true,
                                       "apply_view")
                            .expect("can't create reference");
                    };
                }
                setup_tmp_repo(&env::current_dir().unwrap(), Some(view.as_str()))

            }
            else {
                println!("no view");
                setup_tmp_repo(&env::current_dir().unwrap(), None)
            };

            let mut cmd = Command::new("git");
            cmd.arg("http-backend");
            cmd.current_dir(&view_repo);
            cmd.env("GIT_PROJECT_ROOT", view_repo.to_str().unwrap());
            cmd.env("GIT_DIR", view_repo.to_str().unwrap());
            cmd.env("GIT_HTTP_EXPORT_ALL", "");

            println!("prefix {:?}", prefix);
            let request = request.remove_prefix(&prefix).expect("can't remove prefix");
            println!("URL stripped: {}", request.url());

            println!("done");
            cmd.start_cgi(&request).unwrap()
        })
    });
}

fn update_hook(refname: &str, old: &str, new: &str) -> i32
{
    let scratch = {
        let mut s = String::new();
        File::open(&Path::new("orig"))
            .expect("could not open orig name file")
            .read_to_string(&mut s)
            .expect("could not read orig name");


        let scratch_dir = Path::new(&s);
        let scratch = Scratch::new(&scratch_dir);
        scratch
    };


    let view = {
        let mut s = String::new();
        File::open(&Path::new("view"))
            .expect("could not open view name file")
            .read_to_string(&mut s)
            .expect("could not read view name");

        if s.starts_with(".") {
            return 0;
        }
        let view = SubdirView::new(&Path::new(&s));
        view
    };

    let central_head = scratch.repo.refname_to_id(&refname).expect("no ref: master");


    match scratch.unapply_view(central_head,
                               &view,
                               Oid::from_str(old).expect("can't parse old OID"),
                               Oid::from_str(new).expect("can't parse new OID")) {

        UnapplyView::Done(rewritten) => {
            scratch.repo
                .reference(&refname, rewritten, true, "unapply_view")
                .expect("can't create new reference");
        }
        _ => return 1,
    };

    return 0;
}
