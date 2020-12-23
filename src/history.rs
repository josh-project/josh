#[tracing::instrument(skip(repo, filter))]
pub fn walk(
    repo: &git2::Repository,
    filter: &super::filters::Filter,
    input: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("walk", "spec": super::filters::spec(&filter));
    walk2(
        repo,
        filter,
        input,
        &mut super::filter_cache::Transaction::new(),
    )
}

pub fn walk2(
    repo: &git2::Repository,
    filter: &super::filters::Filter,
    input: git2::Oid,
    transaction: &mut super::filter_cache::Transaction,
) -> super::JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("walk2","spec":super::filters::spec(&filter), "id": input.to_string());
    if super::filters::spec(&filter) == "" {
        return Ok(git2::Oid::zero());
    }

    if transaction.has(&super::filters::spec(&filter), repo, input) {
        return Ok(transaction.get(&super::filters::spec(&filter), input));
    }

    let mut doit = false;
    let incom = repo.find_commit(input)?;

    for p in incom.parent_ids() {
        if !transaction.has(&super::filters::spec(&filter), repo, p) {
            doit = true;
        }
    }

    if !doit {
        /* let t = filter.apply_to_commit(&repo, &incom, transaction)?; */

        let t = super::filters::apply_to_commit(
            &repo,
            &filter,
            &incom,
            transaction,
        )?;

        transaction.insert(&super::filters::spec(&filter), input, t);
        return Ok(t);
    }

    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;
        walk
    };

    log::debug!("starting walk for {:?}", super::filters::spec(&filter));

    for original_commit_id in walk {
        let original_commit_id = original_commit_id?;
        if transaction.has(
            &super::filters::spec(&filter),
            &repo,
            original_commit_id,
        ) {
            continue;
        }
        let original_commit = repo.find_commit(original_commit_id)?;

        let filtered_commit = ok_or!(
            rs_tracing::trace_expr!(
                "apply_to_commit",
                /* filter.apply_to_commit(&repo, &original_commit, transaction), */
                super::filters::apply_to_commit(
                    &repo,
                    &filter,
                    &original_commit,
                    transaction
                ),
                "spec": super::filters::spec(&filter)
            ),
            {
                tracing::error!("cannot apply_to_commit");
                git2::Oid::zero()
            }
        );

        transaction.insert(
            &super::filters::spec(&filter),
            original_commit.id(),
            filtered_commit,
        );
    }

    if !transaction.has(&super::filters::spec(&filter), &repo, input) {
        transaction.insert(
            &super::filters::spec(&filter),
            input,
            git2::Oid::zero(),
        );
    }
    let rewritten = transaction.get(&super::filters::spec(&filter), input);
    return Ok(rewritten);
}
