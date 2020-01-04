use super::*;
use git2::Oid;
use std::env;
use std::path::Path;
use std::sync::{Arc, RwLock};

extern crate reqwest;
extern crate tracing;

use self::tracing::{span, Level};

use std::collections::HashMap;

pub type RepoUpdate = HashMap<String, String>;

fn baseref_and_options(refname: &str) -> (String, String, Vec<String>) {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().unwrap().to_owned();

    let options = if let Some(options) = split.next() {
        options.split(',').map(|x| x.to_string()).collect()
    } else {
        vec![]
    };

    let mut baseref = push_to.to_owned();

    if baseref.starts_with("refs/for") {
        baseref = baseref.replacen("refs/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/drafts") {
        baseref = baseref.replacen("refs/drafts", "refs/heads", 1)
    }
    return (baseref, push_to, options);
}

pub fn process_repo_update(
    repo_update: RepoUpdate,
    _forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> Result<String, String> {
    let ru = {
        let mut ru = repo_update.clone();
        ru.insert("password".to_owned(), "...".to_owned());
    };
    let _trace_s = span!(Level::TRACE, "process_repo_update", repo_update= ?ru);
    let refname = some_or!(repo_update.get("refname"), {
        return Err("".to_owned());
    });
    let viewstr = some_or!(repo_update.get("viewstr"), {
        return Err("".to_owned());
    });
    let old = some_or!(repo_update.get("old"), {
        return Err("".to_owned());
    });
    let new = some_or!(repo_update.get("new"), {
        return Err("".to_owned());
    });
    let username = some_or!(repo_update.get("username"), {
        return Err("".to_owned());
    });
    let password = some_or!(repo_update.get("password"), {
        return Err("".to_owned());
    });
    let remote_url = some_or!(repo_update.get("remote_url"), {
        return Err("".to_owned());
    });
    let base_ns = some_or!(repo_update.get("base_ns"), {
        return Err("".to_owned());
    });
    let git_dir = some_or!(repo_update.get("GIT_DIR"), {
        return Err("".to_owned());
    });
    let git_namespace = some_or!(repo_update.get("GIT_NAMESPACE"), {
        return Err("".to_owned());
    });
    debug!("REPO_UPDATE env ok");

    let scratch = scratch::new(&Path::new(&git_dir));
    /* let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone()); */
    /* let mut fm = view_maps::ViewMaps::new_downstream(forward_maps.clone()); */

    let old = Oid::from_str(old).unwrap();

    let (baseref, push_to, options) = baseref_and_options(refname);
    let josh_merge = options.contains(&"josh-merge".to_string());

    debug!("push options: {:?}", options);
    debug!("XXX josh-merge: {:?}", josh_merge);

    let old = if old == Oid::zero() {
        let rev = format!("refs/namespaces/{}/{}", git_namespace, &baseref);
        let oid = if let Ok(x) = scratch.revparse_single(&rev) {
            x.id()
        } else {
            old
        };
        trace!("push: old oid: {:?}, rev: {:?}", oid, rev);
        oid
    } else {
        trace!("push: old oid: {:?}, refname: {:?}", old, refname);
        old
    };

    let viewobj = views::build_view(&scratch, &viewstr);
    let new_oid = Oid::from_str(&new).expect("can't parse new OID");
    let backward_new_oid = {
        debug!("=== MORE");

        debug!("=== processed_old {:?}", old);

        match scratch::unapply_view(&scratch, backward_maps, &*viewobj, old, new_oid) {
            UnapplyView::Done(rewritten) => {
                debug!("rewritten");
                rewritten
            }
            UnapplyView::BranchDoesNotExist => {
                return Err("branch does not exist on remote".to_owned());
            }
            UnapplyView::RejectMerge(parent_count) => {
                return Err(format!("rejecting merge with {} parents", parent_count));
            }
        }
    };

    /* if !update_ws { */
    /*     let forward_transformed = viewobj.apply_view_to_commit( */
    /*         &scratch, */
    /*         &scratch.find_commit(backward_new_oid).unwrap(), */
    /*         &mut fm, */
    /*         &mut bm, */
    /*         &mut HashMap::new(), */
    /*     ); */

    /*     if new_oid != forward_transformed { */
    /*         return Err("rewritten Mismatch".to_owned()); */
    /*     } */
    /* } */

    let oid_to_push = if josh_merge {
        let rev = format!("refs/namespaces/{}/{}", &base_ns, &baseref);
        let backward_commit = scratch.find_commit(backward_new_oid).unwrap();
        if let Ok(Ok(base_commit)) = scratch.revparse_single(&rev).map(|x| x.peel_to_commit()) {
            let merged_tree = scratch
                .merge_commits(&base_commit, &backward_commit, None)
                .unwrap()
                .write_tree_to(&scratch)
                .unwrap();
            scratch
                .commit(
                    None,
                    &backward_commit.author(),
                    &backward_commit.committer(),
                    &format!("Merge from {}", &viewstr),
                    &scratch.find_tree(merged_tree).unwrap(),
                    &[&base_commit, &backward_commit],
                )
                .unwrap()
        } else {
            return Err("josh_merge failed".to_owned());
        }
    } else {
        backward_new_oid
    };

    let mut options = options;
    options.retain(|x| !x.starts_with("josh-"));
    let options = options;

    let push_with_options = if options.len() != 0 {
        push_to + "%" + &options.join(",")
    } else {
        push_to
    };

    let stderr = ok_or!(
        base_repo::push_head_url(
            &scratch,
            oid_to_push,
            &push_with_options,
            &remote_url,
            &username,
            &password,
            &git_namespace,
        ),
        {
            warn!("REPO_UPDATE push fail");
            return Err("".to_owned());
        }
    );

    return Ok(stderr);
}

pub fn update_hook(refname: &str, old: &str, new: &str) -> i32 {
    let mut repo_update = RepoUpdate::new();
    repo_update.insert("new".to_owned(), new.to_owned());
    repo_update.insert("old".to_owned(), old.to_owned());
    repo_update.insert("refname".to_owned(), refname.to_owned());

    for (env_name, name) in [
        ("JOSH_USERNAME", "username"),
        ("JOSH_PASSWORD", "password"),
        ("JOSH_REMOTE", "remote_url"),
        ("JOSH_BASE_NS", "base_ns"),
        ("JOSH_VIEWSTR", "viewstr"),
        ("GIT_NAMESPACE", "GIT_NAMESPACE"),
    ]
    .iter()
    {
        repo_update.insert(
            name.to_string(),
            env::var(&env_name).expect(&format!("{} not set", &env_name)),
        );
    }

    let scratch = scratch::new(&Path::new(&env::var("GIT_DIR").expect("GIT_DIR not set")));
    repo_update.insert(
        "GIT_DIR".to_owned(),
        scratch.path().to_str().unwrap().to_owned(),
    );

    let port = env::var("JOSH_PORT").expect("JOSH_PORT not set");

    let client = reqwest::Client::builder().timeout(None).build().unwrap();
    let resp = client
        .post(&format!("http://localhost:{}/repo_update", port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(mut r) => {
            if let Ok(body) = r.text() {
                println!("response from upstream:\n {}\n\n", body);
            } else {
                println!("no upstream response");
            }
            if r.status().is_success() {
                return 0;
            } else {
                return 1;
            }
        }
        Err(err) => {
            warn!("/repo_update request failed {:?}", err);
        }
    };
    return 1;
}
