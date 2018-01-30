extern crate centralgithook;
extern crate clap;
extern crate fern;
extern crate git2;
extern crate regex;
extern crate tempdir;

#[macro_use]
extern crate log;

use centralgithook::*;
use regex::Regex;
use rouille::cgi::CgiRun;
use std::env;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::exit;

use centralgithook::virtual_repo;


// #[macro_use]
extern crate rouille;


fn main() { exit(main_ret()); }

fn apply_view_to_branch(scratch: &Scratch, branchname: &str, view: &str) {
    debug!("apply_view_to_branch {}", branchname);
    if let Ok(branch) = scratch.repo.find_branch(branchname, git2::BranchType::Local) {
        let r = branch.into_reference().target().expect("no ref");

        let viewobj = SubdirView::new(&Path::new(&view));
        if let Some(view_commit) = scratch.apply_view(&viewobj, r) {
            println!("applied view to branch {}", branchname);
            scratch.repo
                .reference(&view_ref(&view, &branchname),
                           view_commit,
                           true,
                           "apply_view")
                .expect("can't create reference");
        }
        else {
            println!("can't apply view to branch {}", branchname);
        };
    };
}


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
        return virtual_repo::update_hook(&args[1], &args[2], &args[3]);
    }

    let args = clap::App::new("centralgit-http")
        .arg(clap::Arg::with_name("remote").long("remote").takes_value(true))
        .arg(clap::Arg::with_name("local").long("local").takes_value(true))
        .arg(clap::Arg::with_name("user").long("user").takes_value(true))
        .arg(clap::Arg::with_name("ssh-key").long("ssh-key").takes_value(true))
        .get_matches();

    let user = args.value_of("user").expect("missing user name").to_string();
    let private_key =
        PathBuf::from(args.value_of("ssh-key").expect("missing pirvate ssh key"));

    let base_repo = BaseRepo::create(
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
        &user,
        &private_key);

    base_repo.clone();
    println!("Now listening on localhost:8000");

    rouille::start_server("localhost:8000", move |request| {
        rouille::log(&request, io::stdout(), || {

            let auth = match rouille::input::basic_http_auth(request) {
                Some(a) => a,
                _ => return rouille::Response::basic_http_auth_login_required("realm")
            };

            if !(auth.login == "me" && auth.password == "secret") {
                return rouille::Response::text("bad credentials").with_status_code(403);
            }

            println!("X\nX\nX\nURL: {}", request.url());
            let re = Regex::new(r"(?P<prefix>/.*[.]git)/.*").expect("can't compile regex");

            let prefix = if let Some(caps) = re.captures(&request.url()) {
                caps.name("prefix").expect("can't find name prefix").as_str().to_string()
            }
            else {
                String::new()
            };


            base_repo.fetch_origin_master();
            let scratch = Scratch::new(&base_repo.path);

            let re = Regex::new(r"/(?P<view>.*)[.]git/.*").expect("can't compile regex");


            let view_repo = if let Some(caps) = re.captures(&request.url()) {
                let view = caps.name("view").unwrap();
                println!("VIEW {}", view.as_str());

                let view = caps.name("view").unwrap();

                for branch in scratch.repo.branches(None).unwrap() {
                    apply_view_to_branch(
                        &scratch,
                        &branch.unwrap().0.name().unwrap().unwrap(),
                        &view.as_str());
                }

                virtual_repo::setup_tmp_repo(
                    &base_repo.path,
                    Some(view.as_str()),
                    &user,
                    &private_key)

            }
            else {
                println!("no view");
                virtual_repo::setup_tmp_repo(
                    &base_repo.path,
                    None,
                    &user,
                    &private_key)
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

