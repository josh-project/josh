use git2;

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
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
    FilteredRefRegex,
    r"josh/filtered/(?P<upstream_repo>[^/]*[.]git)/(?P<filter_spec>[^/]*)/.*",
    [upstream_repo, filter_spec]
);

pub fn discover_repos(repo: &git2::Repository) -> JoshResult<Vec<String>> {
    let _trace_s = span!(Level::TRACE, "discover_repos");

    let refname = format!("refs/josh/upstream/*.git/refs/heads/master");

    let mut repos = vec![];

    for reference in repo.references_glob(&refname)? {
        let r = reference?;
        let name = r.name().ok_or(josh_error("reference without name"))?;
        let name = UpstreamRef::from_str(name)
            .ok_or(josh_error("not a ns"))?
            .ns;
        let name = super::from_ns(&name);

        repos.push(name);
    }

    return Ok(repos);
}

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
        let name = UpstreamRef::from_str(name)
            .ok_or(josh_error("not a ns"))?
            .ns;
        let name = super::from_ns(&name);

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
) -> JoshResult<String> {
    let _trace_s = span!(Level::TRACE, "get_info");

    let obj = repo.revparse_single(&format!(
        "refs/josh/upstream/{}/{}",
        &to_ns(&upstream_repo),
        &headref
    ))?;

    let commit = obj.peel_to_commit()?;

    let mut meta = std::collections::HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let filtered =
        super::filters::apply_filter_cached(&repo, &*filter, commit.id())?;

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

    let t = if let Ok(filtered) = repo.find_commit(filtered) {
        json!({
            "commit": filtered.id().to_string(),
            "tree": filtered.tree_id().to_string(),
            "parents": parent_ids(&filtered),
        })
    } else {
        json!({
            "commit": git2::Oid::zero().to_string(),
            "tree": git2::Oid::zero().to_string(),
            "parents": json!([]),
        })
    };

    let s = json!({
        "commit": commit.id().to_string(),
        "tree": commit.tree_id().to_string(),
        "parents": parent_ids(&commit),
        "filtered": t,
    });

    return Ok(serde_json::to_string(&s)?);
}

#[tracing::instrument(skip(repo))]
pub fn refresh_known_filters(
    repo: &git2::Repository,
    known_filters: &KnownViews,
) -> JoshResult<usize> {
    for (upstream_repo, e) in known_filters.iter() {
        info!("background rebuild root: {:?}", upstream_repo);

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
                &*filters::parse(&filter_spec)?,
                &refs,
            )?;
        }
        info!("updated {} refs for {:?}", updated_count, upstream_repo);
    }
    return Ok(0);
}

pub fn spawn_thread(
    repo_path: std::path::PathBuf,
    do_gc: bool,
) -> std::thread::JoinHandle<()> {
    let mut gc_timer = std::time::Instant::now();
    let mut persist_timer =
        std::time::Instant::now() - std::time::Duration::from_secs(60 * 15);
    std::thread::spawn(move || {
        let mut total = 0;
        loop {
            let repo = git2::Repository::init_bare(&repo_path).unwrap();
            let known_filters =
                housekeeping::discover_filter_candidates(&repo).unwrap();
            total += refresh_known_filters(&repo, &known_filters).unwrap_or(0);
            if total > 1000
                || persist_timer.elapsed()
                    > std::time::Duration::from_secs(60 * 15)
            {
                filter_cache::persist(&repo.path());
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
