extern crate git2;

extern crate tempdir;
use std::fs;

use super::*;
use std::path::Path;

pub fn fetch_refs_from_url(
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
        format!("{}://{}@{}", &proto, &username, &rest)
    };
    let cmd = format!("git config --local credential.helper '!f() {{ echo \"password={}\"; }}; f'", &password);
    shell.command(&cmd);

    let cmd = format!("git fetch {} '{}'", &nurl, &spec);
    println!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

    let (stdout, stderr) = shell.command(&cmd);
    if stderr.contains("fatal: Authentication failed") {
        return Err(git2::Error::from_str("auth"));
    }
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
        format!("{}://{}@{}", &proto, &username, &rest)
    };
    let cmd = format!("git config --local credential.helper '!f() {{ echo \"password={}\"; }}; f'", &password);
    shell.command(&cmd);
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let (stdout, stderr) = shell.command(&cmd);
    println!("{}", &stderr);
    println!("{}", &stdout);
}


pub fn create_local(path: &Path)
{
    println!("init base repo: {:?}", path);
    fs::create_dir_all(path);

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
