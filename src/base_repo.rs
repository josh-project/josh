use git2;

use std::fs;

use super::*;
use std::collections::{BTreeSet, HashMap};
use std::env::current_exe;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, span, warn, Level};

pub type KnownViews = HashMap<String, BTreeSet<String>>;

pub fn find_refs(
    repo: &git2::Repository,
    namespace: &str,
    prefix: &str,
    headref: &str,
) -> Vec<(String, String)> {
    if headref != "" {
        return vec![(
            format!("refs/namespaces/{}/{}", &to_ns(prefix), headref),
            format!("refs/namespaces/{}/HEAD", &namespace),
        )];
    }

    let mut refs = vec![];
    let glob = format!("refs/namespaces/{}/*", &to_ns(prefix));
    for refname in repo.references_glob(&glob).unwrap().names() {
        let refname = refname.unwrap();
        let to_ref = refname.replacen(&to_ns(prefix), &namespace, 1);

        if to_ref.contains("/refs/cache-automerge/") {
            continue;
        }
        if to_ref.contains("/refs/changes/") {
            continue;
        }
        if to_ref.contains("/refs/notes/") {
            continue;
        }

        refs.push((refname.to_owned(), to_ref.clone()));
    }

    return refs;
}

pub fn make_view_repo(
    filter: &dyn views::Filter,
    prefix: &str,
    headref: &str,
    namespace: &str,
    br_path: &Path,
    fm: &mut view_maps::ViewMaps,
    bm: &mut view_maps::ViewMaps,
) -> usize {
    let filter_spec = filter.filter_spec();
    let _trace_s =
        span!(Level::TRACE, "make_view_repo", ?filter_spec, ?br_path);

    let scratch = scratch::new(&br_path);

    let refs = find_refs(&scratch, namespace, prefix, headref);
    let to_head = format!("refs/namespaces/{}/HEAD", &namespace);

    let updated_count =
        scratch::apply_view_to_refs(&scratch, &*filter, &refs, fm, bm);

    if headref == "" {
        let mastername =
            format!("refs/namespaces/{}/refs/heads/master", &namespace);
        if let Ok(_) =
            scratch.reference_symbolic(&to_head, &mastername, true, "")
        {
        } else {
            warn!(
                "Can't create reference_symbolic: {:?} -> {:?}",
                &to_head, &mastername
            );
        }
    }

    return updated_count;
}

pub fn run_housekeeping(path: &Path, cmd: &str) -> String {
    let shell = shell::Shell {
        cwd: path.to_owned(),
    };

    let output = "";

    let (stdout, stderr) = shell.command(cmd);
    let output = format!(
        "{}\n\n{}:\nstdout:\n{}\n\nstderr:{}\n",
        output, cmd, stdout, stderr
    );

    return output;
}

pub fn fetch_refs_from_url(
    path: &Path,
    prefix: &str,
    url: &str,
    refs_prefixes: &[&str],
    username: &str,
    password: &str,
) -> Result<(), git2::Error> {
    for refs_prefix in refs_prefixes {
        let spec = format!(
            "+{}:refs/namespaces/{}/{}",
            &refs_prefix,
            to_ns(prefix),
            &refs_prefix
        );

        let shell = shell::Shell {
            cwd: path.to_owned(),
        };
        let nurl = {
            let splitted: Vec<&str> = url.splitn(2, "://").collect();
            let proto = splitted[0];
            let rest = splitted[1];
            format!("{}://{}@{}", &proto, &username, &rest)
        };

        let cmd = format!("git fetch {} '{}'", &nurl, &spec);
        info!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

        let (_stdout, stderr) =
            shell.command_env(&cmd, &[("GIT_PASSWORD", &password)]);
        debug!("fetch_refs_from_url done {:?} {:?} {:?}", cmd, path, stderr);
        if stderr.contains("fatal: Authentication failed") {
            return Err(git2::Error::from_str("auth"));
        }
        if stderr.contains("fatal:") {
            return Err(git2::Error::from_str("error"));
        }
        if stderr.contains("error:") {
            return Err(git2::Error::from_str("error"));
        }
    }
    return Ok(());
}

pub fn push_head_url(
    repo: &git2::Repository,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    username: &str,
    password: &str,
    namespace: &str,
) -> JoshResult<String> {
    let rn = format!("refs/{}", &namespace);

    let spec = format!("{}:{}", &rn, &refname);

    let shell = shell::Shell {
        cwd: repo.path().to_owned(),
    };
    let nurl = {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, &username, &rest)
    };
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let mut fakehead = repo.reference(&rn, oid, true, "push_head_url")?;
    let (stdout, stderr) =
        shell.command_env(&cmd, &[("GIT_PASSWORD", &password)]);
    fakehead.delete()?;
    debug!("{}", &stderr);
    debug!("{}", &stdout);

    let stderr = stderr.replace(&rn, "JOSH_PUSH");

    return Ok(stderr);
}

fn install_josh_hook(scratch_dir: &Path) {
    if !scratch_dir.join("hooks/update").exists() {
        let shell = shell::Shell {
            cwd: scratch_dir.to_path_buf(),
        };
        shell.command("git config http.receivepack true");
        let ce = current_exe().expect("can't find path to exe");
        shell.command("rm -Rf hooks");
        shell.command("mkdir hooks");
        symlink(ce, scratch_dir.join("hooks").join("update"))
            .expect("can't symlink update hook");
        shell.command(&format!(
            "git config credential.helper '!f() {{ echo \"password=\"$GIT_PASSWORD\"\"; }}; f'"
        ));
        shell.command(&"git config gc.auto 0");
    }
}

pub fn create_local(path: &Path) {
    debug!("init base repo: {:?}", path);
    fs::create_dir_all(path).expect("can't create_dir_all");

    match git2::Repository::open(path) {
        Ok(_) => {
            debug!("repo exists");
            install_josh_hook(path);
            return;
        }
        Err(_) => {}
    };

    match git2::Repository::init_bare(path) {
        Ok(_) => {
            info!("repo initialized");
            install_josh_hook(path);
            return;
        }
        Err(_) => {}
    }
}

pub fn discover_views(br_path: &Path, known_views: Arc<RwLock<KnownViews>>) {
    let _trace_s = span!(Level::TRACE, "discover_views", ?br_path);

    let repo = scratch::new(&br_path);

    let refname = format!("refs/namespaces/*.git/refs/heads/master");

    debug!("discover_views {:?}", &br_path);

    if let Ok(mut kn) = known_views.write() {
        for reference in repo.references_glob(&refname).unwrap() {
            let r = reference.unwrap();
            let name = r
                .name()
                .unwrap()
                .to_owned()
                .replace("/refs/heads/master", "")
                .replace("refs/namespaces", "")
                .replace("//", "/");

            {
                let hs = scratch::find_all_views(&r);
                for i in hs {
                    kn.entry(name.clone())
                        .or_insert_with(BTreeSet::new)
                        .insert(i);
                }
            }
        }
    }
}

pub fn get_info(
    view_string: &str,
    prefix: &str,
    rev: &str,
    br_path: &Path,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> String {
    let _trace_s = span!(Level::TRACE, "get_info", ?view_string, ?br_path);

    let scratch = scratch::new(&br_path);

    let mut bm = view_maps::new_downstream(&backward_maps);
    let mut fm = view_maps::new_downstream(&forward_maps);

    let viewobj = build_filter(&view_string);

    let fr = &format!("refs/namespaces/{}/{}", &to_ns(&prefix), &rev);

    let obj = ok_or!(scratch.revparse_single(&fr), {
        ok_or!(scratch.revparse_single(&rev), {
            return format!("rev not found: {:?}", &rev);
        })
    });

    let commit = ok_or!(obj.peel_to_commit(), {
        return format!("not a commit");
    });

    let mut meta = HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let transformed = ok_or!(
        viewobj.apply_to_commit(&scratch, &commit, &mut fm, &mut bm, &mut meta),
        {
            return format!("cannot apply_to_commit");
        }
    );

    let parent_ids = |commit: &git2::Commit| {
        let pids: Vec<_> = commit
            .parent_ids()
            .map(|x| {
                json!({
                    "commit": x.to_string(),
                    "tree": scratch.find_commit(x)
                        .map(|c| { c.tree_id() })
                        .unwrap_or(git2::Oid::zero())
                        .to_string(),
                })
            })
            .collect();
        pids
    };

    let t = if let Ok(transformed) = scratch.find_commit(transformed) {
        json!({
            "commit": transformed.id().to_string(),
            "tree": transformed.tree_id().to_string(),
            "parents": parent_ids(&transformed),
        })
    } else {
        json!({
            "commit": git2::Oid::zero().to_string(),
            "tree": git2::Oid::zero().to_string(),
            "parents": json!([]),
        })
    };

    let s = json!({
        "original": {
            "commit": commit.id().to_string(),
            "tree": commit.tree_id().to_string(),
            "parents": parent_ids(&commit),
        },
        "transformed": t,
    });

    return serde_json::to_string(&s).unwrap_or("Json Error".to_string());
}

fn to_known_view(prefix: &str, filter_spec: &str) -> String {
    return format!(
        "known_views/refs/namespaces/{}/refs/namespaces/{}",
        data_encoding::BASE64URL_NOPAD.encode(prefix.as_bytes()),
        data_encoding::BASE64URL_NOPAD.encode(filter_spec.as_bytes())
    );
}

pub fn spawn_housekeeping_thread(
    known_views: Arc<RwLock<base_repo::KnownViews>>,
    br_path: std::path::PathBuf,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    do_gc: bool,
) -> std::thread::JoinHandle<()> {
    let mut gc_timer = std::time::Instant::now();
    let mut persist_timer =
        std::time::Instant::now() - std::time::Duration::from_secs(60 * 5);
    std::thread::spawn(move || {
        let mut total = 0;
        loop {
            base_repo::discover_views(&br_path.clone(), known_views.clone());
            if let Ok(kn) = known_views.read() {
                for (prefix2, e) in kn.iter() {
                    info!("background rebuild root: {:?}", prefix2);

                    let mut bm = view_maps::new_downstream(&backward_maps);
                    let mut fm = view_maps::new_downstream(&forward_maps);

                    let mut updated_count = 0;

                    for v in e.iter() {
                        tracing::trace!(
                            "background rebuild: {:?} {:?}",
                            prefix2,
                            v
                        );

                        updated_count += base_repo::make_view_repo(
                            &*super::build_filter(&v),
                            &prefix2,
                            "refs/heads/master",
                            &to_known_view(&prefix2, &v),
                            &br_path,
                            &mut fm,
                            &mut bm,
                        );
                    }
                    info!("updated {} refs for {:?}", updated_count, prefix2);

                    let stats = fm.stats();
                    total += fm.stats()["total"];
                    total += bm.stats()["total"];
                    /* debug!( */
                    /*     "forward_maps stats: {}", */
                    /*     toml::to_string_pretty(&stats).unwrap() */
                    /* ); */
                    span!(Level::TRACE, "write_lock bm").in_scope(|| {
                        let mut backward_maps = backward_maps.write().unwrap();
                        backward_maps.merge(&bm);
                    });
                    span!(Level::TRACE, "write_lock fm").in_scope(|| {
                        let mut forward_maps = forward_maps.write().unwrap();
                        forward_maps.merge(&fm);
                    });
                }
            }
            if total > 1000
                || persist_timer.elapsed()
                    > std::time::Duration::from_secs(60 * 15)
            {
                view_maps::persist(
                    &*backward_maps.read().unwrap(),
                    &br_path.join("josh_backward_maps"),
                );
                view_maps::persist(
                    &*forward_maps.read().unwrap(),
                    &br_path.join("josh_forward_maps"),
                );
                total = 0;
                persist_timer = std::time::Instant::now();
            }
            info!(
                "{}",
                base_repo::run_housekeeping(&br_path, &"git count-objects -v")
                    .replace("\n", "  ")
            );
            if do_gc
                && gc_timer.elapsed() > std::time::Duration::from_secs(60 * 60)
            {
                info!(
                    "\n----------\n{}\n----------",
                    base_repo::run_housekeeping(&br_path, &"git repack -adkbn")
                );
                info!(
                    "\n----------\n{}\n----------",
                    base_repo::run_housekeeping(
                        &br_path,
                        &"git count-objects -vH"
                    )
                );
                info!(
                    "\n----------\n{}\n----------",
                    base_repo::run_housekeeping(
                        &br_path,
                        &"git prune --expire=2w"
                    )
                );
                gc_timer = std::time::Instant::now();
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    })
}
