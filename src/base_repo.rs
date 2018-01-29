extern crate git2;

extern crate tempdir;

use std::path::Path;
use std::path::PathBuf;

pub struct BaseRepo
{
    pub path: PathBuf,
}


impl BaseRepo {
    pub fn create(path: &Path) -> BaseRepo 
    {
        return BaseRepo{path: PathBuf::from(&path)};
    }

    pub fn clone(
        &self,
        url: &str,
        user: &str,
        private_key: &Path){

        println!("clone base repo: {:?}", &self.path);

        let mut builder = git2::build::RepoBuilder::new();
        builder.bare(true);

        let mut fetchoptions = git2::FetchOptions::new();

        let mut rcb = git2::RemoteCallbacks::new();
        rcb.credentials(|_,_,_| {
            let cred = git2::Cred::ssh_key(
                user,
                None,
                private_key,
                None
            );
            return cred;
        });
        fetchoptions.remote_callbacks(rcb);
        builder.fetch_options(fetchoptions);


        if let Ok(r) = builder.clone(url, &self.path) { println!("cloned"); }
        else { println!("exists"); }
    }

    pub fn fetch_origin_master(&self) {
        let repo = git2::Repository::open(&self.path).unwrap();
        repo.find_remote("origin").unwrap().fetch(&["master"], None, None);
    }

    pub fn push_origin(&self, refname: &str) {
        let repo = git2::Repository::open(&self.path).unwrap();
        println!("push_origin {}", refname);
        //repo.find_remote("origin").unwrap().push(&[&format!("{}:{}", refname:refname)], None, None);
    }

}
