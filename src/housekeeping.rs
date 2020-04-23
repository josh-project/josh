use git2;

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{info, span, Level};

pub type KnownViews = BTreeMap<String, BTreeSet<String>>;

pub fn default_from_to(
    repo: &git2::Repository,
    namespace: &str,
    upstream_repo: &str,
    filter_spec: &str,
) -> Vec<(String, String)> {
    let mut refs = vec![];

    let glob =
        format!("refs/josh/upstream/{}/refs/heads/*", &to_ns(upstream_repo));
    for refname in repo.references_glob(&glob).unwrap().names() {
        let refname = refname.unwrap();
        let to_ref =
            refname.replacen("refs/josh/upstream", "refs/namespaces", 1);
        let to_ref = to_ref.replacen(&to_ns(upstream_repo), &namespace, 1);
        refs.push((refname.to_owned(), to_ref.clone()));
    }

    let glob =
        format!("refs/josh/upstream/{}/refs/tags/*", &to_ns(upstream_repo));
    for refname in repo.references_glob(&glob).unwrap().names() {
        let refname = refname.unwrap();
        let to_ref =
            refname.replacen("refs/josh/upstream", "refs/namespaces", 1);
        let to_ref = to_ref.replacen(&to_ns(upstream_repo), &namespace, 1);
        refs.push((refname.to_owned(), to_ref.clone()));
    }

    refs.append(&mut memorize_from_to(
        &repo,
        &crate::to_filtered_ref(&upstream_repo, &filter_spec),
        &upstream_repo,
    ));

    return refs;
}

pub fn memorize_from_to(
    repo: &git2::Repository,
    namespace: &str,
    upstream_repo: &str,
) -> Vec<(String, String)> {
    let mut refs = vec![];
    let glob = format!(
        "refs/josh/upstream/{}/refs/heads/master",
        &to_ns(upstream_repo)
    );
    for refname in repo.references_glob(&glob).unwrap().names() {
        let refname = refname.unwrap();
        let to_ref = format!("refs/{}/heads/master", &namespace);

        refs.push((refname.to_owned(), to_ref.clone()));
    }

    return refs;
}

fn run_command(path: &Path, cmd: &str) -> String {
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

super::regex_parsed!(
    UpstreamRef,
    r"refs/josh/upstream/(?P<ns>.*[.]git)/refs/heads/.*",
    [ns]
);

super::regex_parsed!(
    RepoNs,
    r"refs/namespaces/(?P<ns>.*[.]git)/refs/heads/.*",
    [ns]
);

super::regex_parsed!(
    FilteredRefRegex,
    r"josh/filtered/(?P<upstream_repo>[^/]*[.]git)/(?P<filter_spec>[^/]*)/.*",
    [upstream_repo, filter_spec]
);

/**
 * Determine filter specs that are either likely to be requested and/or
 * expensive to build from scratch using heuristics.
 */
pub fn discover_filter_candidates(
    repo: &git2::Repository,
) -> JoshResult<KnownViews> {
    let mut known_filters = KnownViews::new();
    let _trace_s = span!(Level::TRACE, "discover_filter_candidates");

    let refname = format!("refs/josh/upstream/*.git/refs/heads/master");

    for reference in repo.references_glob(&refname)? {
        let r = reference?;
        let name = r.name().ok_or(josh_error("reference without name"))?;
        let name = UpstreamRef::from_str(name).ok_or(josh_error("not a ns"))?.ns;
        let name = super::from_ns(&name);

        let hs = find_all_workspaces_and_subdirectories(&r.peel_to_tree()?)?;

        for i in hs {
            known_filters
                .entry(name.clone())
                .or_insert_with(BTreeSet::new)
                .insert(i);
        }
    }

    let refname = format!("refs/namespaces/*.git/refs/heads/master");
    for reference in repo.references_glob(&refname)? {
        let r = reference?;
        let name = r.name().ok_or(josh_error("reference without name"))?;
        let name = RepoNs::from_str(name).ok_or(josh_error("not a ns"))?.ns;
        let name = name.replace("refs/namespaces", "/");

        let hs = find_all_workspaces_and_subdirectories(&r.peel_to_tree()?)?;

        for i in hs {
            known_filters
                .entry(name.clone())
                .or_insert_with(BTreeSet::new)
                .insert(i);
        }
    }

    let refname = format!("josh/filtered/*.git/*/refs/heads/master");
    for reference in repo.references_glob(&refname)? {
        let r = reference?;
        let name = r.name().ok_or(josh_error("reference without name"))?;
        let filtered =
            FilteredRefRegex::from_str(name).ok_or(josh_error("not a ns"))?;

        known_filters
            .entry(from_ns(&filtered.upstream_repo))
            .or_insert_with(BTreeSet::new)
            .insert(from_ns(&filtered.filter_spec));
    }

    return Ok(known_filters);
}

fn find_all_workspaces_and_subdirectories(
    tree: &git2::Tree,
) -> JoshResult<std::collections::HashSet<String>> {
    let mut hs = std::collections::HashSet::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.name() == Some(&"workspace.josh") {
            hs.insert(format!(":workspace={}", root.trim_matches('/')));
        }
        if root == "" {
            return 0;
        }
        let v = format!(":/{}", root.trim_matches('/'));
        if v.chars().filter(|x| *x == '/').count() < 3 {
            hs.insert(v);
        }

        0
    })?;
    return Ok(hs);
}

pub fn get_info(
    repo: &git2::Repository,
    filter: &dyn filters::Filter,
    upstream_repo: &str,
    headref: &str,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> JoshResult<String> {
    let _trace_s = span!(Level::TRACE, "get_info");

    let mut bm = view_maps::new_downstream(&backward_maps);
    let mut fm = view_maps::new_downstream(&forward_maps);

    let obj = repo.revparse_single(&format!(
        "refs/josh/upstream/{}/{}",
        &to_ns(&upstream_repo),
        &headref
    ))?;

    let commit = obj.peel_to_commit()?;

    let mut meta = std::collections::HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let transformed =
        filter.apply_to_commit(&repo, &commit, &mut fm, &mut bm, &mut meta)?;

    let parent_ids = |commit: &git2::Commit| {
        let pids: Vec<_> = commit
            .parent_ids()
            .map(|x| {
                json!({
                    "commit": x.to_string(),
                    "tree": repo.find_commit(x)
                        .map(|c| { c.tree_id() })
                        .unwrap_or(git2::Oid::zero())
                        .to_string(),
                })
            })
            .collect();
        pids
    };

    let t = if let Ok(transformed) = repo.find_commit(transformed) {
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

    return Ok(serde_json::to_string(&s)?);
}

pub fn refresh_known_filters(
    repo: &git2::Repository,
    known_filters: &KnownViews,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> JoshResult<usize> {
    let mut total = 0;
    for (upstream_repo, e) in known_filters.iter() {
        info!("background rebuild root: {:?}", upstream_repo);

        let mut bm = view_maps::new_downstream(&backward_maps);
        let mut fm = view_maps::new_downstream(&forward_maps);

        let mut updated_count = 0;

        for filter_spec in e.iter() {
            tracing::trace!(
                "background rebuild: {:?} {:?}",
                upstream_repo,
                filter_spec
            );

            let refs = memorize_from_to(
                &repo,
                &to_filtered_ref(&upstream_repo, &filter_spec),
                &upstream_repo,
            );

            updated_count += scratch::apply_filter_to_refs(
                &repo,
                &*filters::parse(&filter_spec),
                &refs,
                &mut fm,
                &mut bm,
            );
        }
        info!("updated {} refs for {:?}", updated_count, upstream_repo);

        total += fm.stats()["total"];
        total += bm.stats()["total"];
        span!(Level::TRACE, "write_lock bm").in_scope(|| {
            let mut backward_maps = backward_maps.write().unwrap();
            backward_maps.merge(&bm);
        });
        span!(Level::TRACE, "write_lock fm").in_scope(|| {
            let mut forward_maps = forward_maps.write().unwrap();
            forward_maps.merge(&fm);
        });
    }
    return Ok(total);
}

pub fn spawn_thread(
    repo_path: std::path::PathBuf,
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
            let repo = git2::Repository::init_bare(&repo_path).unwrap();
            let known_filters =
                housekeeping::discover_filter_candidates(&repo).unwrap();
            total += refresh_known_filters(
                &repo,
                &known_filters,
                forward_maps.clone(),
                backward_maps.clone(),
            )
            .unwrap_or(0);
            if total > 1000
                || persist_timer.elapsed()
                    > std::time::Duration::from_secs(60 * 15)
            {
                view_maps::persist(
                    &*backward_maps.read().unwrap(),
                    &repo.path().join("josh_backward_maps"),
                )
                .ok();
                view_maps::persist(
                    &*forward_maps.read().unwrap(),
                    &repo.path().join("josh_forward_maps"),
                )
                .ok();
                total = 0;
                persist_timer = std::time::Instant::now();
            }
            info!(
                "{}",
                run_command(&repo.path(), &"git count-objects -v")
                    .replace("\n", "  ")
            );
            if do_gc
                && gc_timer.elapsed() > std::time::Duration::from_secs(60 * 60)
            {
                info!(
                    "\n----------\n{}\n----------",
                    run_command(&repo.path(), &"git repack -adkbn")
                );
                info!(
                    "\n----------\n{}\n----------",
                    run_command(&repo.path(), &"git count-objects -vH")
                );
                info!(
                    "\n----------\n{}\n----------",
                    run_command(&repo.path(), &"git prune --expire=2w")
                );
                gc_timer = std::time::Instant::now();
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    })
}
