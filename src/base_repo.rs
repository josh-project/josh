extern crate git2;

extern crate tempdir;

/* use grib::Shell; */
use super::*;
use std::path::Path;
use std::path::PathBuf;

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

    let shell = Shell {
        cwd: path.to_owned(),
    };
    let nurl = {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}:{}@{}", &proto, &username, &password, &rest)
    };
    let cmd = format!("git fetch {} '{}'", &nurl, &spec);
    shell.command(&cmd);
    return Ok(());
}

pub fn push_head_url(
    path: &Path,
    refname: &str,
    url: &str,
    username: &str,
    password: &str)
{
    let spec = format!("HEAD:{}", &refname);

    let shell = Shell {
        cwd: path.to_owned(),
    };
    let nurl = {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}:{}@{}", &proto, &username, &password, &rest)
    };
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let (stdout, stderr) = shell.command(&cmd);
    println!("{}", &stderr);
    println!("{}", &stdout);
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
