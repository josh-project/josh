extern crate git2;

extern crate tempdir;

use std::path::Path;
use std::path::PathBuf;
/* use bobble::Shell; */
use super::*;

#[derive(Clone)]
pub struct BaseRepo
{
    pub path: PathBuf,
    pub url: String,
}


impl BaseRepo
{
    pub fn create(path: &Path, url: &str) -> BaseRepo
    {
        return BaseRepo {
            path: PathBuf::from(&path),
            url: String::from(url),
        };
    }
}

pub fn fetch_origin_master(
    path: &Path,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(), git2::Error>
{
    let spec = "+refs/heads/*:refs/heads/*";
    /* let mut called = false; */
    /* let mut fetchoptions = git2::FetchOptions::new(); */
    /* let rcb = make_remote_callbacks_http(username.to_owned(), password.to_owned(), &mut called); */
    /* fetchoptions.remote_callbacks(rcb); */
    /* let repo = git2::Repository::open(&path).expect("can't open base repo for fetching"); */
    /* let res = repo.remote_anonymous(&url) */
    /*     .expect("can't create anonymous remote") */
    /*     .fetch(&[spec], Some(&mut fetchoptions), None); */
    /* return res; */


    let shell = Shell { cwd: path.to_owned() };
    let splitted: Vec<&str> = url.splitn(2, "://").collect();
    let proto = splitted[0];
    let rest = splitted[1];
    let cmd  = format!("git fetch {}://{}:{}@{} '{}'", &proto, &username, &password, &rest, &spec);
    println!("GIT fetch command: {:?}", cmd);
    shell.command(&cmd);
    return Ok(());
}

pub fn push_head(refname: &str, remote: git2::Remote, username: &str, password: &str)
{
    let mut remote = remote;
    let mut called = false;
    debug!("=== pushing {}:{}", "HEAD", refname);
    let mut po = git2::PushOptions::new();
    let br = make_remote_callbacks_http(username.to_owned(), password.to_owned(), &mut called);
    po.remote_callbacks(br);
    remote
        .push(&[&format!("HEAD:{}", refname)], Some(&mut po))
        .expect("can't find remote");
}

pub fn make_remote_callbacks_ssh<'a>(
    user: &'a str,
    private_key: &'a Path,
) -> git2::RemoteCallbacks<'a>
{
    let mut rcb = git2::RemoteCallbacks::new();
    rcb.credentials(move |_, _, _| {
        let cred = git2::Cred::ssh_key(user, None, private_key, None);
        return cred;
    });
    return rcb;
}

fn make_remote_callbacks_http<'a>(
    user: String,
    pass: String,
    called: &'a mut bool,
) -> git2::RemoteCallbacks<'a>
{
    /* println!("make_remote_callbacks_http {:?} {:?}", &user, &pass); */
    let mut rcb = git2::RemoteCallbacks::new();
    rcb.credentials(move |a, b, c| {
        /* println!("rcb.credentials {:?} {:?} {:?} {:?} {:?}", */
        /* &a, &b, &c, &user, &pass); */
        if *called {
            /* println!("XXXX"); */
            return Err(git2::Error::from_str("wrong credentials"));
        }
        *called = true;
        let cred = git2::Cred::userpass_plaintext(&user, &pass);
        return cred;
    });
    return rcb;
}

pub fn git_clone(path: &Path)
{
    println!("init base repo: {:?}", path);

    match git2::Repository::open(path) {
        Ok(_) => {
            println!("repo exists");
            return;
        }
        Err(_) => {}
    };

    match git2::Repository::init_bare(path) {
        Ok(_) => {
            println!("repo initialized");
            return;
        }
        Err(_) => {}
    }
}
