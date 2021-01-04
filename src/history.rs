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

        return Ok(t);
    }

    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;
        walk
    };

    log::debug!(
        "Walking history for:\n{}\n{:?}",
        filters::pretty(&filter, 4),
        &filter
    );
    let mut n_commits = 0;
    let mut n_misses = transaction.misses;

    transaction.walks += 1;

    for original_commit_id in walk {
        filters::apply_to_commit2(
            &repo,
            &filter,
            &repo.find_commit(original_commit_id?)?,
            transaction,
        )?;

        n_commits += 1;
        if n_commits % 1000 == 0 {
            log::debug!(
                "{} {} commits filtered, {} misses",
                " ->".repeat(transaction.walks),
                n_commits,
                transaction.misses - n_misses,
            );
            n_misses = transaction.misses;
        }
    }

    log::debug!(
        "{} {} commits filtered, {} misses",
        " ->".repeat(transaction.walks),
        n_commits,
        transaction.misses - n_misses,
    );

    transaction.walks -= 1;

    return filters::apply_to_commit2(
        &repo,
        &filter,
        &repo.find_commit(input)?,
        transaction,
    );
}
