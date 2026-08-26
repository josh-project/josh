use crate::*;
use anyhow::anyhow;
use itertools::Itertools;
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;
use tracing::{Level, info, span};

pub type KnownViews = HashMap<String, (gix_hash::ObjectId, BTreeSet<String>)>;

static KNOWN_FILTERS: LazyLock<std::sync::Mutex<KnownViews>> =
    LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn list_refs(
    transaction: &cache::Transaction,
    upstream_repo: &str,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let mut refs = vec![];

    let prefix = ["refs", "josh", "upstream", &to_ns(upstream_repo)]
        .iter()
        .join("/");

    for iter_prefix in [
        format!("{}/refs/heads/", &prefix),
        format!("{}/refs/tags/", &prefix),
    ]
    .iter()
    {
        transaction.for_each_ref_prefixed(iter_prefix, |name, target| {
            let name = name
                .strip_prefix(&prefix)
                .and_then(|name| name.strip_prefix('/'))
                .ok_or_else(|| anyhow!("bug: unexpected result of prefix iteration"))?;

            refs.push((name.to_owned(), target));
            Ok(())
        })?;
    }

    Ok(refs)
}

pub fn remember_filter(upstream_repo: &str, filter_spec: &str) {
    // no need to remember the nop filter since we already keep a reference to
    // the unfiltered branch in refs/josh/upstream
    if filter_spec != ":/"
        && let Ok(mut known_filters) = KNOWN_FILTERS.try_lock()
    {
        let known_f = &mut known_filters
            .entry(upstream_repo.trim_start_matches('/').to_string())
            .or_insert_with(|| {
                (
                    gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    BTreeSet::new(),
                )
            });

        known_f.1.insert(filter_spec.to_string());
    }
}

pub fn default_from_to(
    transaction: &cache::Transaction,
    namespace: &str,
    upstream_repo: &str,
    filter_spec: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut refs = vec![];

    for prefix in [
        format!("refs/josh/upstream/{}/refs/heads/", &to_ns(upstream_repo)),
        format!("refs/josh/upstream/{}/refs/tags/", &to_ns(upstream_repo)),
    ]
    .iter()
    {
        transaction.for_each_ref_prefixed(prefix, |refname, _| {
            let to_ref = refname.replacen("refs/josh/upstream", "refs/namespaces", 1);
            let to_ref = to_ref.replacen(&to_ns(upstream_repo), namespace, 1);
            refs.push((refname.to_owned(), to_ref.clone()));
            Ok(())
        })?;
    }

    // no need to remember the nop filter since we already keep a reference to
    // the unfiltered branch in refs/josh/upstream
    if filter_spec != ":/"
        && let Ok(mut known_filters) = KNOWN_FILTERS.try_lock()
    {
        let known_f = &mut known_filters
            .entry(upstream_repo.trim_start_matches('/').to_string())
            .or_insert_with(|| {
                (
                    gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    BTreeSet::new(),
                )
            });

        known_f.1.insert(filter_spec.to_string());
    }

    Ok(refs)
}

pub fn memorize_from_to(
    transaction: &cache::Transaction,
    namespace: &str,
    upstream_repo: &str,
) -> anyhow::Result<((String, gix_hash::ObjectId), String)> {
    let from = format!("refs/josh/upstream/{}/HEAD", &to_ns(upstream_repo));
    let to_ref = format!("refs/{}/HEAD", &namespace);

    let oid = transaction
        .resolve_ref(&from)?
        .ok_or_else(|| anyhow!("missing ref: {}", from))?;
    Ok(((from, oid), to_ref))
}

regex_parsed!(UpstreamRef, r"refs/josh/upstream/(?P<ns>.*[.]git)/.*", [ns]);

regex_parsed!(
    FilteredRefRegex,
    r"^refs/josh/filtered/(?P<upstream_repo>[^/]*[.]git)/(?P<filter_spec>[^/]*)/HEAD$",
    [upstream_repo, filter_spec]
);

/**
 * Determine filter specs that are either likely to be requested and/or
 * expensive to build from scratch using heuristics.
 */
pub fn discover_filter_candidates(transaction: &cache::Transaction) -> anyhow::Result<()> {
    let mut known_filters = KNOWN_FILTERS.lock().unwrap();
    let trace_s = span!(Level::TRACE, "discover_filter_candidates");
    let _e = trace_s.enter();

    let odb = transaction.odb();
    transaction.for_each_ref_prefixed("refs/josh/upstream/", |name, target| {
        if !name.ends_with(".git/HEAD") {
            return Ok(());
        }
        tracing::trace!("find: {}", name);
        let name = UpstreamRef::from_str(name)
            .ok_or_else(|| anyhow!("not a ns"))?
            .ns;

        let name = from_ns(&name);

        let known_f = &mut known_filters.entry(name.clone()).or_insert_with(|| {
            (
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                BTreeSet::new(),
            )
        });

        if known_f.0 != target {
            let tree = objects::peel_to_tree(odb, target)?;
            let hs = find_all_workspaces_and_subdirectories(odb, tree)?;
            known_f.0 = target;
            for i in hs {
                known_f.1.insert(i);
            }
        }
        Ok(())
    })?;

    transaction.for_each_ref_prefixed("refs/josh/filtered/", |name, _| {
        let Some(filtered) = FilteredRefRegex::from_str(name) else {
            return Ok(());
        };
        tracing::trace!("known: {}", name);
        known_filters
            .entry(from_ns(&filtered.upstream_repo))
            .or_insert_with(|| {
                (
                    gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    BTreeSet::new(),
                )
            })
            .1
            .insert(from_ns(&filtered.filter_spec));
        Ok(())
    })?;

    Ok(())
}

pub fn find_all_workspaces_and_subdirectories(
    src: &impl gix_object::Find,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let _trace_s = span!(Level::TRACE, "find_all_workspaces_and_subdirectories");
    let mut hs = std::collections::HashSet::new();
    objects::walk_tree_preorder(src, tree, &mut |root, entry| {
        if root.is_empty() {
            return Ok(());
        }

        if &entry.filename[..] == b"workspace.josh" {
            hs.insert(format!(":workspace={}", root));
        }
        let v = format!("::{}/", root);
        if v.chars().filter(|x| *x == '/').count() < 3 {
            hs.insert(v);
        }

        Ok(())
    })?;
    Ok(hs)
}

#[tracing::instrument(skip(transaction_mirror, transaction_overlay))]
pub fn refresh_known_filters(
    transaction_mirror: &cache::Transaction,
    transaction_overlay: &cache::Transaction,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let known_filters = KNOWN_FILTERS.lock().unwrap();
    let mut updated_refs = vec![];
    for (upstream_repo, e) in known_filters.iter() {
        info!("background rebuild root: {:?}", upstream_repo);

        for filter_spec in e.1.iter() {
            tracing::trace!("background rebuild: {:?} {:?}", upstream_repo, filter_spec);

            if let Ok((from, to_ref)) = memorize_from_to(
                transaction_mirror,
                &to_filtered_ref(upstream_repo, filter_spec),
                upstream_repo,
            ) {
                let (mut u, _) =
                    filter_refs(transaction_overlay, filter::parse(filter_spec)?, &[from]);
                u[0].0 = to_ref;
                updated_refs.append(&mut u);
            }
        }
    }
    Ok(updated_refs)
}

pub fn get_known_filters() -> anyhow::Result<std::collections::BTreeMap<String, BTreeSet<String>>> {
    Ok(KNOWN_FILTERS
        .lock()
        .unwrap()
        .iter()
        .map(|(repo, (_, filters))| (repo.clone(), filters.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheStack, Expected, TransactionContext};
    use std::sync::Arc;

    #[test]
    fn discovers_filters_from_filtered_refs() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let transaction = TransactionContext::new(dir.path(), Arc::new(CacheStack::new()))
            .open()
            .unwrap();
        let target = transaction.odb().write(gix_object::Kind::Blob, b"filtered");
        let upstream_repo = "discovery.example/repository.git";
        let filter_spec = ":prefix=src";
        let refname = format!("refs/{}/HEAD", to_filtered_ref(upstream_repo, filter_spec));
        transaction
            .update_ref(&refname, Expected::Any, target, "test filter discovery")
            .unwrap();

        // The shared prefix also contains cache refs with a different schema.
        // Discovery must ignore them rather than aborting the scan.
        transaction
            .update_ref(
                "refs/josh/filtered/0123456789abcdef/heads/main",
                Expected::Any,
                target,
                "test unrelated filtered ref",
            )
            .unwrap();

        discover_filter_candidates(&transaction).unwrap();

        let known = get_known_filters().unwrap();
        assert!(
            known
                .get(upstream_repo)
                .is_some_and(|filters| filters.contains(filter_spec))
        );
    }
}
