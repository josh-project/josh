extern crate git2;

use std::fs;

use super::*;
use std::env::current_exe;
use std::os::unix::fs::symlink;
use std::path::Path;

fn to_ns(path: &str) -> String {
    return path.trim_matches('/').replace("/", "/refs/namespaces/");
}

pub fn reset_all(path: &Path) {
    let shell = shell::Shell {
        cwd: path.to_owned(),
    };

    let (_stdout, _stderr) = shell.command(&format!("rm -Rf {:?}", path));
}

pub fn run_housekeeping(path: &Path, cmd: &str) -> String {
    let shell = shell::Shell {
        cwd: path.to_owned(),
    };

    let output = "";

    let (stdout, stderr) = shell.command(cmd);
    let output = format!(
        "{}\n\n{}:\nstdout:\n{}\n\nstderr:{}\n",
        output, cmd, stdout, stderr
    );

    return output;
}

pub fn fetch_refs_from_url(
    path: &Path,
    prefix: &str,
    url: &str,
    refs_prefixes: &[&str],
    username: &str,
    password: &str,
) -> Result<(), git2::Error> {
    for refs_prefix in refs_prefixes {
        let spec = format!(
            "+{}:refs/namespaces/{}/{}",
            &refs_prefix,
            to_ns(prefix),
            &refs_prefix
        );

        let shell = shell::Shell {
            cwd: path.to_owned(),
        };
        let nurl = {
            let splitted: Vec<&str> = url.splitn(2, "://").collect();
            let proto = splitted[0];
            let rest = splitted[1];
            format!("{}://{}@{}", &proto, &username, &rest)
        };

        let cmd = format!("git fetch {} '{}'", &nurl, &spec);
        debug!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

        /* shell.command(&"git prune"); */
        /* shell.command(&"git gc --auto"); */
        shell.command(&"git config gc.auto 0");

        let (_stdout, stderr) = shell.command_env(&cmd, &[("GIT_PASSWORD", &password)]);
        if stderr.contains("fatal: Authentication failed") {
            return Err(git2::Error::from_str("auth"));
        }
        if stderr.contains("fatal:") {
            return Err(git2::Error::from_str("error"));
        }
        if stderr.contains("error:") {
            return Err(git2::Error::from_str("error"));
        }
    }
    return Ok(());
}

pub fn push_head_url(
    repo: &git2::Repository,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    username: &str,
    password: &str,
    namespace: &str,
) -> Result<String, ()> {
    let rn = format!("refs/{}", &namespace);

    let spec = format!("{}:{}", &rn, &refname);

    let shell = shell::Shell {
        cwd: repo.path().to_owned(),
    };
    let nurl = {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, &username, &rest)
    };
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let mut fakehead = repo
        .reference(&rn, oid, true, "push_head_url")
        .expect("can't create reference");
    let (stdout, stderr) = shell.command_env(&cmd, &[("GIT_PASSWORD", &password)]);
    fakehead.delete().expect("fakehead.delete failed");
    debug!("{}", &stderr);
    debug!("{}", &stdout);

    let stderr = stderr.replace(&rn, "JOSH_PUSH");

    return Ok(stderr);
}

fn install_josh_hook(scratch_dir: &Path) {
    let shell = shell::Shell {
        cwd: scratch_dir.to_path_buf(),
    };
    if !scratch_dir.join("hooks/update").exists() {
        shell.command("git config http.receivepack true");
        let ce = current_exe().expect("can't find path to exe");
        shell.command("rm -Rf hooks");
        shell.command("mkdir hooks");
        symlink(ce, scratch_dir.join("hooks").join("update")).expect("can't symlink update hook");
    }
    shell.command(&format!(
        "git config credential.helper '!f() {{ echo \"password=\"$GIT_PASSWORD\"\"; }}; f'"
    ));
}

pub fn create_local(path: &Path) {
    info!("init base repo: {:?}", path);
    fs::create_dir_all(path).expect("can't create_dir_all");

    match git2::Repository::open(path) {
        Ok(_) => {
            info!("repo exists");
            install_josh_hook(path);
            return;
        }
        Err(_) => {}
    };

    match git2::Repository::init_bare(path) {
        Ok(_) => {
            info!("repo initialized");
            install_josh_hook(path);
            return;
        }
        Err(_) => {}
    }
}
