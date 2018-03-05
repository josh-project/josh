extern crate git2;

extern crate tempdir;

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

    pub fn make_remote_callbacks_http<'a>(
        user: String,
        pass: String,
        called: &'a mut bool,
    ) -> git2::RemoteCallbacks<'a>
    {
        println!("make_remote_callbacks_http {:?} {:?}", &user, &pass);
        let mut rcb = git2::RemoteCallbacks::new();
        rcb.credentials(move |a, b, c| {
            /* println!("rcb.credentials {:?} {:?} {:?} {:?} {:?}", */
            /* &a, &b, &c, &user, &pass); */
            if *called {
                println!("XXXX");
                return Err(git2::Error::from_str("wrong credentials"));
            }
            *called = true;
            let cred = git2::Cred::userpass_plaintext(&user, &pass);
            return cred;
        });
        return rcb;
    }

    pub fn git_clone(&self)
    {
        println!("init base repo: {:?}", &self.path);

        match git2::Repository::open(&self.path) {
            Ok(_) => {
                println!("repo exists");
                return;
            }
            Err(_) => {}
        };

        match git2::Repository::init_bare(&self.path) {
            Ok(_) => {
                println!("repo initialized");
                return;
            }
            Err(_) => {}
        }
    }

    pub fn fetch_origin_master(&self, username: &str, password: &str) -> Result<(), git2::Error>
    {
        let mut called = false;
        let mut fetchoptions = git2::FetchOptions::new();
        let rcb = BaseRepo::make_remote_callbacks_http(
            username.to_owned(),
            password.to_owned(),
            &mut called,
        );
        fetchoptions.remote_callbacks(rcb);
        let repo = git2::Repository::open(&self.path).expect("can't open base repo for fetching");
        let res = repo.remote_anonymous(&self.url)
            .expect("can't create anonymous remote")
            .fetch(&["+refs/heads/*:refs/heads/*"], Some(&mut fetchoptions), None);
        return res;
    }
}
