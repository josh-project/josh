extern crate git2;
extern crate clap;
extern crate env_logger;

use std::path::Path;
use std::path::PathBuf;
use super::RepoHost;

pub struct Gerrit
{
    path: PathBuf,
    prefix: String,
    central_name: String,
    automation_user: String,
    gerrit_host: String,
    port: String,
}

impl Gerrit
{
    pub fn new(git_dir: &Path,
               central_name: &str,
               automation_user: &str,
               gerrit_host: &str,
               port: &str)
        -> Option<(PathBuf, Self)>
    {
        let mut root = PathBuf::new();
        let mut found_central = false;

        let mut p = git_dir;
        while !p.join("bin").join("gerrit.sh").exists() {
            if p.join(&format!("{}.git", central_name)).exists() {
                root = p.to_path_buf();
                found_central = true;
            }
            p = p.parent().expect("can't find gerrit root");
        }

        if !found_central {
            return None;
        }

        let path = p.to_path_buf();

        let prefix = root.strip_prefix(&path.join("git")).unwrap().to_path_buf();

        debug!("Gerrit prefix: {:?}", prefix);

        Some((path.clone(),
              Gerrit {
            path: path,
            prefix: format!("{}/", prefix.as_os_str().to_str().unwrap()),
            central_name: central_name.to_string(),
            automation_user: automation_user.to_string(),
            gerrit_host: gerrit_host.to_string(),
            port: port.to_string(),
        }))
    }
}

impl RepoHost for Gerrit
{
    fn local_path(&self, module_path: &str) -> String
    {
        let root = self.path.as_os_str().to_str().expect("local_path: to_str failed");
        format!("{}/git/{}{}.git", root, self.prefix, module_path)
    }

    fn remote_url(&self, module_path: &str) -> String
    {
        format!("ssh://{}@{}:{}/{}{}.git",
                self.automation_user,
                self.gerrit_host,
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

    fn prefix(&self) -> &str
    {
        &self.prefix
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
                    let name = &name.as_str()[from..name.len() - 4].trim_left_matches("/");
                    repos.push(name.to_string());
                    continue;
                }
            }
            repos = find_repos(root, &path, repos);
        }
    }
    return repos;
}
