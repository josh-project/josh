#![deny(warnings)]
extern crate centralgithook;
extern crate clap;
extern crate fern;
extern crate git2;
extern crate regex;
extern crate tempdir;

#[macro_use]
extern crate lazy_static;

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

lazy_static! {
    static ref PREFIX_RE: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)/.*").expect("can't compile regex");
    static ref VIEW_RE: Regex =
        Regex::new(r"/(?P<view>.*)[.]git/.*").expect("can't compile regex");
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

            match rouille::input::basic_http_auth(request) {
                Some(auth) => if !(auth.login == "me" && auth.password == "secret") {
                    return rouille::Response::text("bad credentials").with_status_code(403);
                },
                _ => return rouille::Response::basic_http_auth_login_required("realm")
            };

            base_repo.fetch_origin_master();
            let view_repo = make_view_repo(&request.url(), &base_repo.path,  &user, &private_key);

            let prefix = if let Some(caps) = PREFIX_RE.captures(&request.url()) {
                caps.name("prefix").expect("can't find name prefix").as_str().to_string()
            }
            else { String::new() };
            let request = request.remove_prefix(&prefix).expect("can't remove prefix");
            run_git_http_backend(request, view_repo)
        })
    });
}

fn make_view_repo(url: &str, base: &Path, user: &str, private_key: &Path) -> PathBuf
{
    let view_string = if let Some(caps) = VIEW_RE.captures(&url) {
        caps.name("view").unwrap().as_str().to_owned()
    }
    else { ".".to_owned() };

    println!("VIEW {}", &view_string);

    let scratch = Scratch::new(&base);
    for branch in scratch.repo.branches(None).unwrap() {
        scratch.apply_view_to_branch(
            &branch.unwrap().0.name().unwrap().unwrap(),
            &view_string);
    }

    virtual_repo::setup_tmp_repo(
        &base,
        &view_string,
        &user,
        &private_key)
}


fn run_git_http_backend(request: rouille::Request, view_repo: PathBuf) -> rouille::Response
{
    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&view_repo);
    cmd.env("GIT_PROJECT_ROOT", view_repo.to_str().unwrap());
    cmd.env("GIT_DIR", view_repo.to_str().unwrap());
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.start_cgi(&request).unwrap()
}

