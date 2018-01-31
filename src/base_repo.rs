extern crate git2;

extern crate tempdir;

use std::path::Path;
use std::path::PathBuf;

pub struct BaseRepo
{
    pub path: PathBuf,
    url: String,
    user: String,
    private_key: PathBuf,
}


impl BaseRepo {
    pub fn create(
        path: &Path,
        url: &str,
        user: &str,
        private_key: &Path
        ) -> BaseRepo
    {
        return BaseRepo{
            path: PathBuf::from(&path),
            url: String::from(url),
            user: String::from(user),
            private_key: PathBuf::from(private_key),
        };
    }

    pub fn make_remote_callbacks<'a>(
        user: &'a str,
        private_key: &'a Path
    ) -> git2::RemoteCallbacks<'a>
    {
        let mut rcb = git2::RemoteCallbacks::new();
        rcb.credentials(move |_,_,_| {
            let cred = git2::Cred::ssh_key(
                user,
                None,
                private_key,
                None
            );
            return cred;
        });
        return rcb;
    }

    pub fn clone(&self)
    {

        println!("clone base repo: {:?}", &self.path);

        let mut builder = git2::build::RepoBuilder::new();
        builder.bare(true);

        let mut fetchoptions = git2::FetchOptions::new();

        let rcb = BaseRepo::make_remote_callbacks(&self.user, &self.private_key);
        fetchoptions.remote_callbacks(rcb);
        builder.fetch_options(fetchoptions);


        if let Ok(_) = builder.clone(&self.url, &self.path) { println!("cloned"); }
        else { println!("exists"); }
    }

    pub fn fetch_origin_master(&self) {
        let mut fetchoptions = git2::FetchOptions::new();
        let rcb = BaseRepo::make_remote_callbacks(&self.user, &self.private_key);
        fetchoptions.remote_callbacks(rcb);
        let repo = git2::Repository::open(&self.path).unwrap();
        repo.find_remote("origin")
                .unwrap()
            .fetch(&["+refs/heads/*:refs/heads/*"], Some(&mut fetchoptions), None)
                .expect("can't fetch base repo")
;
    }

    /* pub fn push_origin(&self, refname: &str) { */
    /*     let repo = git2::Repository::open(&self.path).unwrap(); */
    /*     println!("push_origin {}", refname); */
    /*     //repo.find_remote("origin").unwrap().push(&[&format!("{}:{}", refname:refname)], None, None); */
    /* } */

}
