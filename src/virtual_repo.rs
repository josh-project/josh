use super::*;
use git2::Oid;
use std::env;
use std::path::Path;
use std::sync::{Arc, RwLock};

extern crate reqwest;
extern crate tracing;

use self::tracing::{debug, span, trace, warn, Level};

use std::collections::HashMap;

pub type RepoUpdate = HashMap<String, String>;

fn baseref_and_options(
    refname: &str,
) -> JoshResult<(String, String, Vec<String>)> {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().ok_or(josh_error("no next"))?.to_owned();

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
    return Ok((baseref, push_to, options));
}

pub fn process_repo_update(
    repo_update: RepoUpdate,
    _forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> Result<String, JoshError> {
    let ru = {
        let mut ru = repo_update.clone();
        ru.insert("password".to_owned(), "...".to_owned());
    };
    let _trace_s = span!(Level::TRACE, "process_repo_update", repo_update= ?ru);
    let refname = repo_update.get("refname").ok_or(josh_error(""))?;
    let viewstr = repo_update.get("viewstr").ok_or(josh_error(""))?;
    let old = repo_update.get("old").ok_or(josh_error(""))?;
    let new = repo_update.get("new").ok_or(josh_error(""))?;
    let username = repo_update.get("username").ok_or(josh_error(""))?;
    let password = repo_update.get("password").ok_or(josh_error(""))?;
    let remote_url = repo_update.get("remote_url").ok_or(josh_error(""))?;
    let base_ns = repo_update.get("base_ns").ok_or(josh_error(""))?;
    let git_dir = repo_update.get("GIT_DIR").ok_or(josh_error(""))?;
    let git_ns = repo_update.get("GIT_NAMESPACE").ok_or(josh_error(""))?;
    debug!("REPO_UPDATE env ok");

    let scratch = scratch::new(&Path::new(&git_dir));

    let old = Oid::from_str(old)?;

    let (baseref, push_to, options) = baseref_and_options(refname)?;
    let josh_merge = options.contains(&"josh-merge".to_string());

    debug!("push options: {:?}", options);
    debug!("XXX josh-merge: {:?}", josh_merge);

    let old = if old == Oid::zero() {
        let rev = format!("refs/namespaces/{}/{}", git_ns, &baseref);
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
    let new_oid = Oid::from_str(&new)?;
    let backward_new_oid = {
        debug!("=== MORE");

        debug!("=== processed_old {:?}", old);

        match scratch::unapply_view(
            &scratch,
            backward_maps,
            &*viewobj,
            old,
            new_oid,
        ) {
            UnapplyView::Done(rewritten) => {
                debug!("rewritten");
                rewritten
            }
            UnapplyView::BranchDoesNotExist => {
                return Err(josh_error("branch does not exist on remote"));
            }
            UnapplyView::RejectMerge(parent_count) => {
                return Err(josh_error(&format!(
                    "rejecting merge with {} parents",
                    parent_count
                )));
            }
        }
    };

    let oid_to_push = if josh_merge {
        let rev = format!("refs/namespaces/{}/{}", &base_ns, &baseref);
        let backward_commit = scratch.find_commit(backward_new_oid)?;
        if let Ok(Ok(base_commit)) =
            scratch.revparse_single(&rev).map(|x| x.peel_to_commit())
        {
            let merged_tree = scratch
                .merge_commits(&base_commit, &backward_commit, None)?
                .write_tree_to(&scratch)?;
            scratch.commit(
                None,
                &backward_commit.author(),
                &backward_commit.committer(),
                &format!("Merge from {}", &viewstr),
                &scratch.find_tree(merged_tree)?,
                &[&base_commit, &backward_commit],
            )?
        } else {
            return Err(josh_error("josh_merge failed"));
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

    return base_repo::push_head_url(
        &scratch,
        oid_to_push,
        &push_with_options,
        &remote_url,
        &username,
        &password,
        &git_ns,
    );
}

pub fn update_hook(refname: &str, old: &str, new: &str) -> JoshResult<i32> {
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
        repo_update.insert(name.to_string(), env::var(&env_name)?);
    }

    let scratch = scratch::new(&Path::new(&env::var("GIT_DIR")?));
    repo_update.insert(
        "GIT_DIR".to_owned(),
        scratch
            .path()
            .to_str()
            .ok_or(josh_error("GIT_DIR not set"))?
            .to_owned(),
    );

    let port = env::var("JOSH_PORT")?;

    let client = reqwest::Client::builder().timeout(None).build()?;
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
                return Ok(0);
            } else {
                return Ok(1);
            }
        }
        Err(err) => {
            warn!("/repo_update request failed {:?}", err);
        }
    };
    return Ok(1);
}
