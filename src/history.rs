use super::*;

#[tracing::instrument(skip(repo, filter))]
pub fn walk(
    repo: &git2::Repository,
    filter: &filters::Filter,
    input: git2::Oid,
) -> JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("walk", "spec": filters::spec(&filter));
    walk2(
        repo,
        filter,
        input,
        &mut filter_cache::Transaction::new(&repo),
    )
}

pub fn walk2(
    repo: &git2::Repository,
    filter: &filters::Filter,
    input: git2::Oid,
    transaction: &mut filter_cache::Transaction,
) -> JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("walk2","spec":filters::spec(&filter), "id": input.to_string());

    let input_commit = ok_or!(repo.find_commit(input), {
        return Ok(git2::Oid::zero());
    });

    if let Some(oid) = transaction.get(&filters::spec(&filter), input) {
        return Ok(oid);
    }

    let mut doit = false;

    for p in input_commit.parent_ids() {
        if transaction.get(&filters::spec(&filter), p) == None {
            doit = true;
        }
    }

    if !doit {
        let t = filters::apply_to_commit2(
            &repo,
            &filter,
            &input_commit,
            transaction,
        )?;

        transaction.insert(&filters::spec(&filter), input, t);
        return Ok(t);
    }

    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;
        walk
    };

    log::debug!("Walking history for {:?}", filters::spec(&filter));
    let mut n_commits = 0;

    for original_commit_id in walk {
        let original_commit_id = original_commit_id?;
        if transaction.get(&filters::spec(&filter), original_commit_id) != None
        {
            continue;
        }
        let original_commit = repo.find_commit(original_commit_id)?;

        let filtered_commit = ok_or!(
            rs_tracing::trace_expr!(
                "apply_to_commit",
                filters::apply_to_commit2(
                    &repo,
                    &filter,
                    &original_commit,
                    transaction
                ),
                "spec": filters::spec(&filter)
            ),
            {
                tracing::error!("cannot apply_to_commit");
                git2::Oid::zero()
            }
        );

        n_commits += 1;
        transaction.insert(
            &filters::spec(&filter),
            original_commit.id(),
            filtered_commit,
        );
        if n_commits % 1000 == 0 {
            log::debug!("    -> {} commits filtered", n_commits);
        }
    }

    log::debug!("    -> {} commits filtered", n_commits);

    return Ok(
        if let Some(oid) = transaction.get(&filters::spec(&filter), input) {
            oid
        } else {
            transaction.insert(
                &filters::spec(&filter),
                input,
                git2::Oid::zero(),
            );
            git2::Oid::zero()
        },
    );
}
