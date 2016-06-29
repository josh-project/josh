extern crate git2;
extern crate clap;
extern crate env_logger;

use std::path::Path;
use std::path::PathBuf;
use scratch::RepoHost;

pub struct Gerrit
{
    pub path: PathBuf,
    pub prefix: String,
    central_name: String,
    automation_user: String,
    port: String,
}

impl Gerrit
{
    pub fn new(git_dir: &Path, central_name: &str, automation_user: &str, port: &str) -> Self
    {
        let mut p = git_dir;
        while !p.join(&format!("{}.git", central_name)).exists() {
            p = p.parent().expect("can't find gerrit git root");
        }

        let root = p.to_path_buf();

        while !p.join("bin").join("gerrit.sh").exists() {
            p = p.parent().expect("can't find gerrit root");
        }

        let path = p.to_path_buf();
        let p = p.join("git");

        let prefix = root.strip_prefix(&p).unwrap().to_path_buf();

        println!("Gerrit prefix: {:?}", prefix);

        Gerrit {
            path: path,
            prefix: format!("{}/", prefix.as_os_str().to_str().unwrap()),
            central_name: central_name.to_string(),
            automation_user: automation_user.to_string(),
            port: port.to_string(),
        }
    }
}

impl RepoHost for Gerrit
{
    fn fetch_url(&self, module_path: &str) -> String
    {
        if let Some(root) = self.path.as_os_str().to_str() {
            format!("{}/git/{}{}.git", root, self.prefix, module_path)
        }
        else {
            self.remote_url(module_path)
        }
    }

    fn remote_url(&self, module_path: &str) -> String
    {
        format!("ssh://{}@gerrit-test-git:{}/{}{}.git",
                self.automation_user,
                self.port,
                self.prefix,
                module_path)
    }

    fn projects(&self) -> Vec<String>
    {
        let path = self.path.join("git").join(&self.prefix);
        find_repos(&path, &path, vec![])
    }

    fn central(&self) -> &str
    {
        &self.central_name
    }
}

pub fn find_repos(root: &Path, path: &Path, mut repos: Vec<String>) -> Vec<String>
{
    if let Ok(children) = path.read_dir() {
        for child in children {
            let path = child.unwrap().path();

            let name = format!("{}", &path.to_str().unwrap());
            if let Some(last) = path.extension() {
                if last == "git" {
                    let from = root.to_str().unwrap().len();
                    let name = &name.as_str()[from..name.len()-4].trim_left_matches("/");
                    repos.push(name.to_string());
                    continue;
                }
            }
            repos = find_repos(root, &path, repos);
        }
    }
    return repos;
}
