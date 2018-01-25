extern crate git2;

extern crate tempdir;

use std::path::Path;
use std::path::PathBuf;



pub fn do_clone_base(
    host: &str,
    repo: &str,
    user: &str,
    private_key: &Path) -> tempdir::TempDir {
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


    let r = builder.clone(
        &format!("ssh://{}@{}/{}", user, host, repo), &td.path()).expect("can't clone");
    for remote in r.remotes().unwrap().iter()
    {
        println!("remote: {:?}", remote);
    }
    return td;
}
