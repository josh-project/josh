use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReverseLineage {
    Unspecified,
    Linear,
    Merge,
    Orphan,
}

#[derive(Clone, Copy)]
struct ReverseContext {
    tree: gix_hash::ObjectId,
    parent_tree: gix_hash::ObjectId,
    commits: Option<(gix_hash::ObjectId, gix_hash::ObjectId)>,
    lineage: ReverseLineage,
    synthetic_base: bool,
}

#[derive(Clone, Copy)]
struct ResolvedFilter {
    forward: Filter,
    inverted: Option<Filter>,
}

impl ResolvedFilter {
    fn new(filter: Filter) -> Self {
        Self {
            forward: filter,
            inverted: None,
        }
    }

    fn inverted(self) -> anyhow::Result<Filter> {
        match self.inverted {
            Some(inverted) => Ok(inverted),
            None => invert(self.forward),
        }
    }
}

/// Calculate a tree with minimal differences from `parent_tree`
/// such that `apply(unapply(tree, parent_tree)) == tree`.
///
/// `commits` carries the commit being reconstructed and the specific original parent used to
/// resolve per-commit filters (`:rev` / `:hook`). `parent_tree` is the surrounding original parent
/// or synthetic base tree and need not be that parent commit's tree. Callers that only ever pass
/// tree-reversible filters pass `None` for `commits`.
pub fn unapply(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: gix_hash::ObjectId,
    parent_tree: gix_hash::ObjectId,
    commits: Option<(gix_hash::ObjectId, gix_hash::ObjectId)>,
) -> anyhow::Result<gix_hash::ObjectId> {
    unapply_with_lineage(
        transaction,
        filter,
        tree,
        parent_tree,
        commits,
        ReverseLineage::Unspecified,
        false,
    )
}

pub(crate) fn unapply_with_lineage(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: gix_hash::ObjectId,
    parent_tree: gix_hash::ObjectId,
    commits: Option<(gix_hash::ObjectId, gix_hash::ObjectId)>,
    lineage: ReverseLineage,
    synthetic_base: bool,
) -> anyhow::Result<gix_hash::ObjectId> {
    unapply_impl(
        transaction,
        filter,
        ReverseContext {
            tree,
            parent_tree,
            commits,
            lineage,
            synthetic_base,
        },
    )
}

struct UnapplyResult {
    tree: gix_hash::ObjectId,
    ownership_used: bool,
}

fn chain_ownership_candidate(filter: Filter, context: ReverseContext) -> bool {
    context.lineage == ReverseLineage::Linear
        && !context.synthetic_base
        && matches!(peel_op(filter), Op::Chain(_))
        && contains_compose(filter)
}

fn unapply_impl(
    transaction: &cache::Transaction,
    filter: Filter,
    context: ReverseContext,
) -> anyhow::Result<gix_hash::ObjectId> {
    if chain_ownership_candidate(filter, context) {
        let result = unapply_impl_with_status(transaction, filter, context)?;
        if result.ownership_used {
            checked_forward_roundtrip(transaction, filter, result.tree, context.tree)?;
            return Ok(result.tree);
        }
        if let Some(static_result) = unapply_static(transaction, filter, context)? {
            if static_result.ownership_used {
                checked_forward_roundtrip(transaction, filter, static_result.tree, context.tree)?;
            }
            return Ok(static_result.tree);
        }
        return Ok(result.tree);
    }
    unapply_impl_with_status(transaction, filter, context).map(|result| result.tree)
}

fn unapply_static(
    transaction: &cache::Transaction,
    filter: Filter,
    context: ReverseContext,
) -> anyhow::Result<Option<UnapplyResult>> {
    let Ok(inverted) = invert(filter) else {
        return Ok(None);
    };
    let normalized = invert(inverted)?;
    let filter = ResolvedFilter {
        forward: normalized,
        inverted: Some(inverted),
    };
    let original_filter = ResolvedFilter {
        forward: normalized,
        inverted: Some(inverted),
    };
    reverse_strip_overlay_status(transaction, filter, original_filter, context).map(Some)
}

fn unapply_impl_with_status(
    transaction: &cache::Transaction,
    filter: Filter,
    context: ReverseContext,
) -> anyhow::Result<UnapplyResult> {
    // A `:rev(...)` filter has no static inverse (`invert` returns `Err`, like `:workspace` /
    // `:stored` / `:starlark`), so this generic path is automatically skipped for it and it falls
    // through to the per-commit handler / chain recursion below.
    if !chain_ownership_candidate(filter, context)
        && let Some(result) = unapply_static(transaction, filter, context)?
    {
        return Ok(result);
    }

    // `peel_op` (not `to_op`) so a `:~(...)[...]` meta wrapper around e.g. `:rev(...)` is seen
    // through: the meta options (history flags) only affect the forward direction, and the
    // per-commit reverse handler needs the wrapped operator.
    if let Some(result) = unapply_per_rev_filter(transaction, &peel_op(filter), context)? {
        return Ok(result);
    }

    if let Op::Chain(filters) = peel_op(filter) {
        // Split into first and rest, unapply recursively
        let (first, rest) = match filters.split_first() {
            Some((first, rest)) => (first, rest),
            None => {
                return Ok(UnapplyResult {
                    tree: context.tree,
                    ownership_used: false,
                });
            }
        };

        if rest.is_empty() {
            return unapply_impl_with_status(transaction, *first, context);
        }

        let rest_chain = to_filter(Op::Chain(rest.to_vec()));

        // Compute filtered_parent_tree for the first filter. `:rev`/`:hook` select their sub-filter
        // from commit identity rather than the tree, so they cannot be applied at tree level:
        // resolve the parent's concrete sub-filter and apply that instead. Without commit context
        // (tree-only callers) it is unresolvable, so this is an error rather than a silent identity.
        let first_op = peel_op(*first);
        let filtered_parent_tree = if is_commit_resolved_filter(&first_op) {
            let (_, parent) = context.commits.ok_or_else(|| {
                anyhow!(
                    "cannot reverse a per-commit filter (`:rev`/`:hook`) without commit context"
                )
            })?;
            let parent_filter = resolve_commit_filter(transaction, &first_op, parent)?;
            apply(
                transaction,
                parent_filter,
                Rewrite::from_tree(context.parent_tree),
            )?
            .tree_id()
        } else {
            let first_normalized = if let Ok(first_inverted) = invert(*first) {
                invert(first_inverted)?
            } else {
                *first
            };
            apply(
                transaction,
                first_normalized,
                Rewrite::from_tree(context.parent_tree),
            )?
            .tree_id()
        };

        // Recursively unapply: first unapply the rest, then unapply first
        let rest = unapply_impl_with_status(
            transaction,
            rest_chain,
            ReverseContext {
                parent_tree: filtered_parent_tree,
                ..context
            },
        )?;
        let first = unapply_impl_with_status(
            transaction,
            *first,
            ReverseContext {
                tree: rest.tree,
                ..context
            },
        )?;
        return Ok(UnapplyResult {
            tree: first.tree,
            ownership_used: first.ownership_used || rest.ownership_used,
        });
    }

    Err(anyhow!("filter cannot be unapplied"))
}

/// Reproduce the out-of-filter content of the original parent/base tree (strip) and overlay the
/// reconstructed in-filter content of the filtered tree. The forward filters may differ when a
/// per-commit filter resolves to different sub-filters for the commit and its parent.
fn reverse_strip_overlay_status(
    transaction: &cache::Transaction,
    filter: ResolvedFilter,
    original_filter: ResolvedFilter,
    context: ReverseContext,
) -> anyhow::Result<UnapplyResult> {
    let filtered = apply(
        transaction,
        original_filter.forward,
        Rewrite::from_tree(context.parent_tree),
    )?;
    if let Some(owned) = ownership_unapply(
        transaction,
        filter,
        original_filter,
        context,
        filtered.tree_id(),
    )? {
        return Ok(UnapplyResult {
            tree: owned,
            ownership_used: true,
        });
    }
    let matching = apply(transaction, original_filter.inverted()?, filtered)?;
    let stripped = tree::subtract(transaction, context.parent_tree, matching.tree_id())?;
    let x = apply(
        transaction,
        filter.inverted()?,
        Rewrite::from_tree(context.tree),
    )?;

    Ok(UnapplyResult {
        tree: tree::overlay(transaction, x.tree_id(), stripped)?,
        ownership_used: false,
    })
}

fn ownership_unapply(
    transaction: &cache::Transaction,
    filter: ResolvedFilter,
    original_filter: ResolvedFilter,
    context: ReverseContext,
    parent_filtered: gix_hash::ObjectId,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    if context.lineage != ReverseLineage::Linear
        || context.synthetic_base
        || filter.forward != original_filter.forward
    {
        return Ok(None);
    }
    let Op::Compose(filters) = peel_op(original_filter.forward) else {
        return Ok(None);
    };

    let metadata = ownership_compose_metadata(transaction, &filters, context.parent_tree)?;
    if metadata.tree != parent_filtered {
        return Ok(None);
    }

    let delta = match tree::tree_delta(transaction, parent_filtered, context.tree) {
        Ok(delta) => delta,
        Err(_) => return Ok(None),
    };
    if !delta.content_edits_only {
        return Ok(None);
    }
    let unchanged = delta.before == tree::empty_id() && delta.after == tree::empty_id();

    let changed = tree::overlay(transaction, delta.before, delta.after)?;
    let mut remaining = changed;
    let mut affected = Vec::new();
    for index in (0..metadata.branches.len()).rev() {
        let matched = tree::intersect(transaction, metadata.branches[index].visible, remaining)?;
        remaining = tree::subtract(transaction, remaining, matched)?;
        if matched != tree::empty_id() {
            affected.push((index, matched));
        }
    }

    let mut destinations = tree::empty_id();
    let mut has_overlap = false;
    for branch in &metadata.branches {
        if tree::intersect(transaction, destinations, branch.applied)? != tree::empty_id() {
            has_overlap = true;
            break;
        }
        destinations = tree::overlay(transaction, destinations, branch.applied)?;
    }
    let has_nested_compose = if unchanged {
        filters.iter().any(|filter| contains_compose(*filter))
    } else {
        affected
            .iter()
            .any(|(index, _)| contains_compose(filters[*index]))
    };
    if !has_overlap && !has_nested_compose {
        return Ok(None);
    }
    if unchanged {
        return Ok(Some(context.parent_tree));
    }
    if affected
        .iter()
        .any(|(index, _)| !ownership_branch_is_safe(filters[*index]))
    {
        return Ok(None);
    }
    if remaining != tree::empty_id() {
        return Err(anyhow!(
            "filtered change is not owned by any compose branch"
        ));
    }

    let mut source = context.parent_tree;
    let mut source_changes = tree::empty_id();
    for (index, matched) in affected {
        let branch = &metadata.branches[index];
        let old = tree::intersect(transaction, delta.before, matched)?;
        let new = tree::intersect(transaction, delta.after, matched)?;
        let branch_view = tree::overlay(
            transaction,
            new,
            tree::subtract(transaction, branch.applied, old)?,
        )?;
        let candidate = unapply_impl(
            transaction,
            filters[index],
            ReverseContext {
                tree: branch_view,
                ..context
            },
        )?;
        let source_delta = tree::tree_delta(transaction, context.parent_tree, candidate)?;
        let coverage = ownership_source_coverage(
            transaction,
            filters[index],
            context.parent_tree,
            branch.applied,
            branch.applied,
            Some(branch.source_coverage),
        )?;
        let changed = checked_source_delta(transaction, source_delta, coverage, source_changes)?;
        source_changes = tree::overlay(transaction, source_changes, changed)?;
        source = tree::overlay(
            transaction,
            source_delta.after,
            tree::subtract(transaction, source, source_delta.before)?,
        )?;
    }

    checked_forward_roundtrip(transaction, filter.forward, source, context.tree)?;
    Ok(Some(source))
}

fn ownership_compose_metadata(
    transaction: &cache::Transaction,
    filters: &[Filter],
    parent_tree: gix_hash::ObjectId,
) -> anyhow::Result<tree::ComposeMetadata> {
    let mut applied = Vec::with_capacity(filters.len());
    for branch in filters {
        applied.push(apply(transaction, *branch, Rewrite::from_tree(parent_tree))?.tree_id());
    }
    tree::compose_with_metadata(
        transaction,
        filters.iter().zip(applied.iter().copied()).collect(),
    )
}

fn ownership_source_coverage(
    transaction: &cache::Transaction,
    filter: Filter,
    input_tree: gix_hash::ObjectId,
    view: gix_hash::ObjectId,
    applied: gix_hash::ObjectId,
    known_coverage: Option<gix_hash::ObjectId>,
) -> anyhow::Result<gix_hash::ObjectId> {
    match peel_op(filter) {
        Op::Compose(filters) => {
            let metadata = ownership_compose_metadata(transaction, &filters, input_tree)?;
            if metadata.tree != applied {
                return Err(anyhow!(
                    "compose metadata mismatch during source coverage computation"
                ));
            }
            let mut coverage = tree::empty_id();
            for (index, branch) in metadata.branches.iter().enumerate() {
                let branch_view = tree::intersect(transaction, branch.visible, view)?;
                if branch_view == tree::empty_id() {
                    continue;
                }
                let branch_coverage = ownership_source_coverage(
                    transaction,
                    filters[index],
                    input_tree,
                    branch_view,
                    branch.applied,
                    Some(branch.source_coverage),
                )?;
                coverage = tree::overlay(transaction, coverage, branch_coverage)?;
            }
            Ok(coverage)
        }
        Op::Chain(filters) => {
            let mut inputs = Vec::with_capacity(filters.len());
            let mut outputs = Vec::with_capacity(filters.len());
            let mut current = input_tree;
            for stage in &filters {
                inputs.push(current);
                current = apply(transaction, *stage, Rewrite::from_tree(current))?.tree_id();
                outputs.push(current);
            }
            if current != applied {
                return Err(anyhow!(
                    "chain metadata mismatch during source coverage computation"
                ));
            }

            let mut coverage = view;
            for index in (0..filters.len()).rev() {
                coverage = ownership_source_coverage(
                    transaction,
                    filters[index],
                    inputs[index],
                    coverage,
                    outputs[index],
                    None,
                )?;
            }
            Ok(coverage)
        }
        _ => {
            if view == applied
                && let Some(coverage) = known_coverage
            {
                return Ok(coverage);
            }
            Ok(apply(transaction, invert(filter)?, Rewrite::from_tree(view))?.tree_id())
        }
    }
}

fn contains_compose(filter: Filter) -> bool {
    match peel_op(filter) {
        Op::Compose(_) => true,
        Op::Chain(filters) => filters.into_iter().any(contains_compose),
        Op::Meta(_, filter) => contains_compose(filter),
        _ => false,
    }
}

fn ownership_branch_is_safe(filter: Filter) -> bool {
    match peel_op(filter) {
        Op::Nop | Op::Empty | Op::File(_, _) | Op::Prefix(_) | Op::Subdir(_) | Op::Pattern(_) => {
            true
        }
        Op::Compose(filters) | Op::Chain(filters) => {
            filters.into_iter().all(ownership_branch_is_safe)
        }
        Op::Meta(_, filter) => ownership_branch_is_safe(filter),
        _ => false,
    }
}

fn checked_source_delta(
    transaction: &cache::Transaction,
    delta: tree::TreeDelta,
    coverage: gix_hash::ObjectId,
    previous_changes: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if !delta.content_edits_only {
        return Err(anyhow!(
            "ownership branch produced unsupported source changes"
        ));
    }
    let before_outside = tree::subtract(transaction, delta.before, coverage)?;
    let after_outside = tree::subtract(transaction, delta.after, coverage)?;
    if before_outside != tree::empty_id() || after_outside != tree::empty_id() {
        return Err(anyhow!(
            "ownership branch produced changes outside its source coverage"
        ));
    }

    let changed = tree::overlay(transaction, delta.before, delta.after)?;
    if previous_changes != tree::empty_id()
        && tree::intersect(transaction, changed, previous_changes)? != tree::empty_id()
    {
        return Err(anyhow!(
            "ownership branches produced overlapping source deltas"
        ));
    }
    Ok(changed)
}

fn checked_forward_roundtrip(
    transaction: &cache::Transaction,
    filter: Filter,
    source: gix_hash::ObjectId,
    expected: gix_hash::ObjectId,
) -> anyhow::Result<()> {
    let roundtrip = apply(transaction, filter, Rewrite::from_tree(source))?.tree_id();
    if roundtrip != expected {
        return Err(anyhow!("ownership forward validation failed"));
    }
    Ok(())
}

/// Resolve the concrete sub-filter of a per-commit filter whose selector lives in *commit identity*
/// rather than the tree: `:rev(...)` (commit ancestry) and `:hook` (hook lookup). These mirror the
/// forward `Op::Rev`/`Op::Hook` paths and, unlike `:workspace`/`:stored`/`:starlark`, cannot be
/// resolved from a tree. Returns `Err` for any other op (callers only reach it for rev/hook).
fn resolve_commit_filter(
    transaction: &cache::Transaction,
    op: &Op,
    commit: gix_hash::ObjectId,
) -> anyhow::Result<Filter> {
    match op {
        Op::Rev(revs) => get_rev_filter(transaction, commit, revs),
        Op::Hook(hook) => transaction.lookup_filter_hook(hook, commit),
        _ => Err(anyhow!("not a per-commit (rev/hook) filter")),
    }
}

/// Whether `op` is a per-commit filter whose selector lives in commit identity (`:rev` / `:hook`),
/// so reversing it requires commit context and it cannot be applied at tree level.
fn is_commit_resolved_filter(op: &Op) -> bool {
    matches!(op, Op::Rev(_) | Op::Hook(_))
}

fn unapply_per_rev_filter(
    transaction: &cache::Transaction,
    op: &Op,
    context: ReverseContext,
) -> anyhow::Result<Option<UnapplyResult>> {
    if is_commit_resolved_filter(op) {
        // `:rev`/`:hook` select their sub-filter from commit identity (mirroring the forward
        // `Op::Rev`/`Op::Hook` paths): resolve it for the commit being reconstructed and for the
        // specific parent whose tree is `parent_tree`. Unlike the tree-resolved per-commit filters
        // below, this cannot be read from a tree, so without commit context it is an error rather
        // than a silent identity.
        let (commit, parent) = context.commits.ok_or_else(|| {
            anyhow!("cannot reverse a per-commit filter (`:rev`/`:hook`) without commit context")
        })?;
        let filter = resolve_commit_filter(transaction, op, commit)?;
        let original_filter = resolve_commit_filter(transaction, op, parent)?;
        let original_filter = ResolvedFilter::new(original_filter);
        let filter = ResolvedFilter::new(filter);
        return Ok(Some(reverse_strip_overlay_status(
            transaction,
            filter,
            original_filter,
            context,
        )?));
    }

    match op {
        Op::Workspace(path) => {
            let odb = transaction.odb();
            let tree = pre_process_tree(transaction, context.tree)?;
            let tree_reader = tree::read_tree(transaction, odb, tree)?;
            let parent_reader = tree::read_tree(transaction, odb, context.parent_tree)?;
            let workspace = get_filter(
                transaction,
                odb,
                tree,
                &tree_reader,
                Path::new("workspace.josh"),
            );
            let original_workspace = get_filter(
                transaction,
                odb,
                context.parent_tree,
                &parent_reader,
                &path.join("workspace.josh"),
            );

            let root = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = to_filter(Op::File(
                Path::new("workspace.josh").to_owned(),
                Path::new("workspace.josh").to_owned(),
            ));
            let wsj_file = root.chain(wsj_file);
            let filter = compose(&[wsj_file, compose(&[workspace, root])]);
            let original_filter = compose(&[wsj_file, compose(&[original_workspace, root])]);
            let original_filter = ResolvedFilter::new(original_filter);
            let filter = ResolvedFilter::new(filter);
            let context = ReverseContext { tree, ..context };
            Ok(Some(reverse_strip_overlay_status(
                transaction,
                filter,
                original_filter,
                context,
            )?))
        }
        Op::Stored(path) => {
            let odb = transaction.odb();
            let stored_path = path.with_added_extension("josh");
            let tree_reader = tree::read_tree(transaction, odb, context.tree)?;
            let parent_reader = tree::read_tree(transaction, odb, context.parent_tree)?;
            let stored = get_filter(transaction, odb, context.tree, &tree_reader, &stored_path);
            let original_stored = get_filter(
                transaction,
                odb,
                context.parent_tree,
                &parent_reader,
                &stored_path,
            );

            let sj_file = Filter::new().file(stored_path.clone());
            let filter = compose(&[sj_file, stored]);
            let original_filter = compose(&[sj_file, original_stored]);
            let original_filter = ResolvedFilter::new(original_filter);
            let filter = ResolvedFilter::new(filter);
            Ok(Some(reverse_strip_overlay_status(
                transaction,
                filter,
                original_filter,
                context,
            )?))
        }
        Op::Starlark(path, subfilter) => {
            let odb = transaction.odb();
            let tree_reader = tree::read_tree(transaction, odb, context.tree)?;
            let parent_reader = tree::read_tree(transaction, odb, context.parent_tree)?;
            let filter = get_starlark(
                transaction,
                odb,
                context.tree,
                &tree_reader,
                path,
                *subfilter,
            );
            let original_filter = get_starlark(
                transaction,
                odb,
                context.parent_tree,
                &parent_reader,
                path,
                *subfilter,
            );
            let original_filter = ResolvedFilter::new(original_filter);
            let filter = ResolvedFilter::new(filter);
            Ok(Some(reverse_strip_overlay_status(
                transaction,
                filter,
                original_filter,
                context,
            )?))
        }
        _ => Ok(None),
    }
}

fn pre_process_tree(
    transaction: &cache::Transaction,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    let path = Path::new("workspace.josh");
    let ws_file = tree::get_blob(transaction, odb, tree, path);
    let parsed = filter::parse(&ws_file)?;

    if invert(parsed).is_err() {
        return Err(anyhow!("Invalid workspace: not reversible"));
    }

    let mut blob = String::new();
    if let Ok(c) = get_comments(&ws_file)
        && !c.is_empty()
    {
        blob = c;
    }
    let blob = &format!("{}{}\n", &blob, pretty(parsed, 0));

    let tree = tree::insert_oid(
        odb,
        tree,
        path,
        odb.write(gix_object::Kind::Blob, blob.as_bytes()),
        0o100644, // Should this handle filemode?
    )?;

    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_tree(repo: &gix::Repository, path: &str, content: &str) -> gix_hash::ObjectId {
        let mut builder = repo.edit_tree(tree::empty_id()).unwrap();
        let oid = josh_gix_ext::write_blob(&repo.objects, content.as_bytes()).unwrap();
        builder
            .upsert(path, gix::objs::tree::EntryKind::Blob, oid)
            .unwrap();
        builder.write().unwrap().detach()
    }

    #[test]
    fn ownership_rejects_source_delta_outside_branch_coverage() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let context = cache::TransactionContext::new(
            repo.path(),
            std::sync::Arc::new(cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let parent = build_tree(&repo, "src/file.txt", "old");
        let candidate = build_tree(&repo, "src/file.txt", "new");
        let coverage = build_tree(&repo, "other/file.txt", "coverage");
        let delta = tree::tree_delta(&transaction, parent, candidate).unwrap();

        let error =
            checked_source_delta(&transaction, delta, coverage, tree::empty_id()).unwrap_err();
        assert!(
            error.to_string().contains("outside its source coverage"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn ownership_rejects_overlapping_source_deltas() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let context = cache::TransactionContext::new(
            repo.path(),
            std::sync::Arc::new(cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let parent = build_tree(&repo, "src/file.txt", "old");
        let candidate = build_tree(&repo, "src/file.txt", "new");
        let delta = tree::tree_delta(&transaction, parent, candidate).unwrap();
        let previous = checked_source_delta(&transaction, delta, parent, tree::empty_id()).unwrap();

        let error = checked_source_delta(&transaction, delta, parent, previous).unwrap_err();
        assert!(
            error.to_string().contains("overlapping source deltas"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn ownership_requires_forward_roundtrip() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let context = cache::TransactionContext::new(
            repo.path(),
            std::sync::Arc::new(cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let parent = build_tree(&repo, "src/file.txt", "old");
        let different = build_tree(&repo, "src/file.txt", "different");

        checked_forward_roundtrip(&transaction, Filter::new(), parent, parent).unwrap();
        let error =
            checked_forward_roundtrip(&transaction, Filter::new(), parent, different).unwrap_err();
        assert!(
            error.to_string().contains("forward validation failed"),
            "unexpected error: {error}"
        );
    }
}
