extern crate git2;

extern crate tempdir;

use std::path::PathBuf;



pub fn do_clone_base() -> tempdir::TempDir {
    let td = tempdir::TempDir::new("centralgit").expect("failed to create tempdir");

    println!("clone base repo: {:?}", td.path());

    let mut builder = git2::build::RepoBuilder::new();
    builder.bare(true);

    let mut fetchoptions = git2::FetchOptions::new();

    let mut rcb = git2::RemoteCallbacks::new();
    rcb.credentials(|_,_,_| {
        /* let cred = git2::Cred::userpass_plaintext( */
        /*     "christian.schilling", */
        /*     "7b5KX2ivtyvxPOcG5lnM2DGvMUKWtOlcOx26DEqqDA"); */
        /* let cred = git2::Cred::ssh_key_from_agent( */
        /*     "christian" */

        /* ); */
        let cred = git2::Cred::ssh_key(
            "christian.schilling",
            Some(&PathBuf::from("/Users/christian/.ssh/id_rsa.pub")),
            &PathBuf::from("/Users/christian/.ssh/id_rsa"),
            None
        );
        return cred;
    });
    fetchoptions.remote_callbacks(rcb);
    builder.fetch_options(fetchoptions);


    let r = builder.clone("ssh://christian.schilling@gerrit:29418/bsw/central.git", &td.path()).expect("can't clone");
    for remote in r.remotes().unwrap().iter()
    {

        println!("remote: {:?}", remote);
    }
    return td;
}
