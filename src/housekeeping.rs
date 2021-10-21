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

    for glob in [
        format!("refs/josh/upstream/{}/refs/heads/*", &to_ns(upstream_repo)),
        format!("refs/josh/upstream/{}/refs/tags/*", &to_ns(upstream_repo)),
    ]
    .iter()
    {
        for refname in repo.references_glob(glob).unwrap().names() {
            let refname = refname.unwrap();
            let to_ref = refname.replacen("refs/josh/upstream", "refs/namespaces", 1);
            let to_ref = to_ref.replacen(&to_ns(upstream_repo), namespace, 1);
            refs.push((refname.to_owned(), to_ref.clone()));
        }
    }
    refs.append(&mut memorize_from_to(
        repo,
        &crate::to_filtered_ref(upstream_repo, filter_spec),
        upstream_repo,
    ));

    refs
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

    refs
}

fn run_command(path: &Path, cmd: &str) -> String {
    let shell = shell::Shell {
        cwd: path.to_owned(),
    };

    let output = "";

    let (stdout, stderr, _) = shell.command(cmd);
    let output = format!(
        "{}\n\n{}:\nstdout:\n{}\n\nstderr:{}\n",
        output, cmd, stdout, stderr
    );

    output
}

regex_parsed!(
    UpstreamRef,
    r"refs/josh/upstream/(?P<ns>.*[.]git)/refs/heads/.*",
    [ns]
);

regex_parsed!(
    FilteredRefRegex,
    r"josh/filtered/(?P<upstream_repo>[^/]*[.]git)/(?P<filter_spec>[^/]*)/.*",
    [upstream_repo, filter_spec]
);

/**
 * Determine filter specs that are either likely to be requested and/or
 * expensive to build from scratch using heuristics.
 */
pub fn discover_filter_candidates(transaction: &cache::Transaction) -> JoshResult<KnownViews> {
    let repo = transaction.repo();
    let mut known_filters = KnownViews::new();
    let trace_s = span!(Level::TRACE, "discover_filter_candidates");
    let _e = trace_s.enter();

    let refname = "refs/josh/upstream/*.git/refs/heads/*".to_string();

    for reference in repo.references_glob(&refname)? {
        let r = reference?;
        let name = r.name().ok_or(josh_error("reference without name"))?;
        let name = UpstreamRef::from_str(name)
            .ok_or(josh_error("not a ns"))?
            .ns;
        let name = from_ns(&name);
        tracing::trace!("find: {}", name);

        let hs = find_all_workspaces_and_subdirectories(&r.peel_to_tree()?)?;

        for i in hs {
            known_filters
                .entry(name.clone())
                .or_insert_with(BTreeSet::new)
                .insert(i);
        }
    }

    let refname = "josh/filtered/*.git/*/refs/heads/*".to_string();
    for reference in repo.references_glob(&refname)? {
        let r = reference?;
        let name = r.name().ok_or(josh_error("reference without name"))?;
        tracing::trace!("known: {}", name);
        let filtered = FilteredRefRegex::from_str(name).ok_or(josh_error("not a ns"))?;

        known_filters
            .entry(from_ns(&filtered.upstream_repo))
            .or_insert_with(BTreeSet::new)
            .insert(from_ns(&filtered.filter_spec));
    }

    Ok(known_filters)
}

pub fn find_all_workspaces_and_subdirectories(
    tree: &git2::Tree,
) -> JoshResult<std::collections::HashSet<String>> {
    let _trace_s = span!(Level::TRACE, "find_all_workspaces_and_subdirectories");
    let mut hs = std::collections::HashSet::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.name() == Some("workspace.josh") {
            hs.insert(format!(":workspace={}", root.trim_matches('/')));
        }
        if root.is_empty() {
            return 0;
        }
        let v = format!(":/{}", root.trim_matches('/'));
        if v.chars().filter(|x| *x == '/').count() < 3 {
            hs.insert(v);
        }

        0
    })?;
    Ok(hs)
}

pub fn get_info(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    headref: &str,
) -> JoshResult<String> {
    let _trace_s = span!(Level::TRACE, "get_info");

    let obj = transaction
        .repo()
        .revparse_single(&transaction.refname(headref))?;

    let commit = obj.peel_to_commit()?;

    let mut meta = std::collections::HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let filtered = filter::apply_to_commit(filter, &commit, transaction)?;

    let parent_ids = |commit: &git2::Commit| {
        commit
            .parent_ids()
            .map(|x| {
                json!({
                    "commit": x.to_string(),
                    "tree": transaction.repo().find_commit(x)
                        .map(|c| { c.tree_id() })
                        .unwrap_or(git2::Oid::zero())
                        .to_string(),
                })
            })
            .collect::<Vec<_>>()
    };

    let t = if let Ok(filtered) = transaction.repo().find_commit(filtered) {
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

    Ok(serde_json::to_string(&s)?)
}

#[tracing::instrument(skip(transaction))]
pub fn refresh_known_filters(
    transaction: &cache::Transaction,
    known_filters: &KnownViews,
) -> JoshResult<usize> {
    for (upstream_repo, e) in known_filters.iter() {
        let t = transaction.try_clone()?;
        info!("background rebuild root: {:?}", upstream_repo);

        let mut updated_count = 0;

        for filter_spec in e.iter() {
            tracing::trace!("background rebuild: {:?} {:?}", upstream_repo, filter_spec);

            let refs = memorize_from_to(
                t.repo(),
                &to_filtered_ref(upstream_repo, filter_spec),
                upstream_repo,
            );

            updated_count += filter_refs(&t, filter::parse(filter_spec)?, &refs)?;
        }
        info!("updated {} refs for {:?}", updated_count, upstream_repo);
    }
    Ok(0)
}

pub fn run(repo_path: &std::path::Path, do_gc: bool) -> JoshResult<()> {
    let transaction = cache::Transaction::open(repo_path, None)?;
    let known_filters = housekeeping::discover_filter_candidates(&transaction)?;
    refresh_known_filters(&transaction, &known_filters)?;
    info!(
        "{}",
        run_command(transaction.repo().path(), "git count-objects -v").replace("\n", "  ")
    );
    if do_gc {
        info!(
            "\n----------\n{}\n----------",
            run_command(transaction.repo().path(), "git repack -adkbn --threads=1")
        );
        info!(
            "\n----------\n{}\n----------",
            run_command(transaction.repo().path(), "git count-objects -vH")
        );
        info!(
            "\n----------\n{}\n----------",
            run_command(transaction.repo().path(), "git prune --expire=2w")
        );
    }
    Ok(())
}
