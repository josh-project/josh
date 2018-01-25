extern crate git2;

extern crate tempdir;

use std::path::Path;
use std::path::PathBuf;

pub struct BaseRepo
{
    pub td: tempdir::TempDir,
}


impl BaseRepo {
    pub fn clone(
        url: &str,
        user: &str,
        private_key: &Path) -> BaseRepo {
        let td = tempdir::TempDir::new("centralgit").expect("failed to create tempdir");

        println!("clone base repo: {:?}", td.path());

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


        let r = builder.clone(url, &td.path()).expect("can't clone");
        for remote in r.remotes().unwrap().iter()
        {
            println!("remote: {:?}", remote);
        }
        return BaseRepo{ td: td };
    }

    pub fn fetch_origin_master(&self) {
        let repo = git2::Repository::open(self.td.path()).unwrap();
        repo.find_remote("origin").unwrap().fetch(&["master"], None, None);
    }

}
