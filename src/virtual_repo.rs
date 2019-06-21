use super::*;
use super::view_maps::*;
use git2::Oid;
use std::env;
use std::path::Path;

extern crate reqwest;

use self::reqwest::Client;

use std::collections::HashMap;

pub type RepoUpdate = HashMap<String, String>;

pub fn process_repo_update(
    repo_update: RepoUpdate,
    backward_maps: &ViewMaps,
) -> Result<String, ()> {
    let ru = {
        let mut ru = repo_update.clone();
        ru.insert("password".to_owned(), "...".to_owned());
    };
    trace_scoped!("process_repo_update", "repo_update": ru);
    let refname = some_or!(repo_update.get("refname"), {
        return Err(());
    });
    let viewstr = some_or!(repo_update.get("viewstr"), {
        return Err(());
    });
    let old = some_or!(repo_update.get("old"), {
        return Err(());
    });
    let new = some_or!(repo_update.get("new"), {
        return Err(());
    });
    let username = some_or!(repo_update.get("username"), {
        return Err(());
    });
    let password = some_or!(repo_update.get("password"), {
        return Err(());
    });
    let remote_url = some_or!(repo_update.get("remote_url"), {
        return Err(());
    });
    let git_dir = some_or!(repo_update.get("GIT_DIR"), {
        return Err(());
    });
    let git_namespace = some_or!(repo_update.get("GIT_NAMESPACE"), {
        return Err(());
    });
    println!("REPO_UPDATE env ok");

    let scratch = scratch::new(&Path::new(&git_dir));
    let new_oid = {
        let viewobj = build_view(&viewstr);
        debug!("=== MORE");

        let old = Oid::from_str(old).unwrap();
        debug!("=== processed_old {:?}", old);

        match scratch::unapply_view(
            &scratch,
            backward_maps,
            &*viewobj,
            old,
            Oid::from_str(&new).expect("can't parse new OID"),
        ) {
            UnapplyView::Done(rewritten) => {
                debug!("rewritten");
                rewritten
            }
            _ => {
                debug!("rewritten ERROR");
                return Err(());
            }
        }
    };

    let stderr = ok_or!(
        base_repo::push_head_url(
            &scratch,
            new_oid,
            &refname,
            &remote_url,
            &username,
            &password,
            &git_namespace,
        ),
        {
            println!("REPO_UPDATE push fail");
            return Err(());
        }
    );

    return Ok(stderr);
}

pub fn update_hook(refname: &str, old: &str, new: &str) -> i32 {
    let mut repo_update = RepoUpdate::new();
    repo_update.insert("new".to_owned(), new.to_owned());
    repo_update.insert("old".to_owned(), old.to_owned());
    repo_update.insert("refname".to_owned(), refname.to_owned());
    repo_update.insert(
        "username".to_owned(),
        env::var("JOSH_USERNAME").expect("JOSH_USERNAME not set"),
    );
    repo_update.insert(
        "password".to_owned(),
        env::var("JOSH_PASSWORD").expect("JOSH_PASSWORD not set"),
    );
    repo_update.insert(
        "remote_url".to_owned(),
        env::var("JOSH_REMOTE").expect("JOSH_REMOTE not set"),
    );
    repo_update.insert(
        "viewstr".to_owned(),
        env::var("JOSH_VIEWSTR").expect("JOSH_VIEWSTR not set"),
    );

    repo_update.insert(
        "GIT_NAMESPACE".to_owned(),
        env::var("GIT_NAMESPACE").expect("GIT_NAMESPACE not set"),
    );

    let scratch = scratch::new(&Path::new(&env::var("GIT_DIR").expect("GIT_DIR not set")));
    repo_update.insert(
        "GIT_DIR".to_owned(),
        scratch.path().to_str().unwrap().to_owned(),
    );

    let port = env::var("JOSH_PORT").expect("JOSH_PORT not set");

    let client = reqwest::Client::new();
    let resp = client
        .post(&format!("http://localhost:{}/repo_update", port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(mut r) => {
            if r.status().is_success() {
                if let Ok(body) = r.text() {
                    println!("response from upstream:\n {}\n\n", body);
                } else {
                    println!("no upstream response");
                }
                return 0;
            }
        }
        Err(err) => {
            println!("/repo_update request failed {:?}", err);
        }
    };
    return 1;
}
