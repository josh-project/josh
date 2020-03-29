use super::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::*;

pub fn get_info(
    view_string: &str,
    prefix: &str,
    rev: &str,
    repo: &git2::Repository,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> String {
    let _trace_s =
        span!(Level::TRACE, "get_info", ?view_string, br_path = ?repo.path());

    let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone());
    let mut fm = view_maps::ViewMaps::new_downstream(forward_maps.clone());

    let viewobj = build_view(&repo, &view_string);

    let fr = &format!("refs/namespaces/{}/{}", &to_ns(&prefix), &rev);

    let obj = ok_or!(repo.revparse_single(&fr), {
        ok_or!(repo.revparse_single(&rev), {
            return format!("rev not found: {:?}", &rev);
        })
    });

    let commit = ok_or!(obj.peel_to_commit(), {
        return format!("not a commit");
    });

    let mut meta = HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let transformed = viewobj
        .apply_view_to_commit(&repo, &commit, &mut fm, &mut bm, &mut meta);

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

    return serde_json::to_string(&s).unwrap_or("Json Error".to_string());
}
