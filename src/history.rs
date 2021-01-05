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

pub fn find_original(
    repo: &git2::Repository,
    bm: &mut std::collections::HashMap<git2::Oid, git2::Oid>,
    filter: &filters::Filter,
    contained_in: git2::Oid,
    filtered: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if contained_in == git2::Oid::zero() {
        return Ok(git2::Oid::zero());
    }
    if let Some(original) = bm.get(&filtered) {
        return Ok(*original);
    }
    let oid = super::history::walk(&repo, &filter, contained_in)?;
    if oid != git2::Oid::zero() {
        bm.insert(contained_in, oid);
    }
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(contained_in)?;

    for original in walk {
        let original = repo.find_commit(original?)?;
        if filtered == filters::apply_to_commit(&repo, &filter, &original)? {
            bm.insert(filtered, original.id());
            return Ok(original.id());
        }
    }

    return Ok(git2::Oid::zero());
}
