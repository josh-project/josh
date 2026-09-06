use anyhow::{Context, anyhow};
use josh_filter::{ObjectKind, ObjectResolver};
use std::collections::BTreeMap;
use std::str::FromStr;

/// A named compose argument supplied as `NAME=REVSPEC`.
///
/// Argument values are currently Git single-revision expressions. The
/// `--arg` spelling leaves room for additional value types in the future.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentBinding {
    /// Name referenced by filter object expressions.
    pub name: String,
    /// Argument value resolved eagerly during plan construction.
    pub value: String,
}

fn validate_argument_binding(name: &str, value: &str) -> anyhow::Result<()> {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
    anyhow::ensure!(
        valid_start && valid_rest,
        "invalid argument binding name `{name}`"
    );
    anyhow::ensure!(name != "input", "argument binding name `input` is reserved");
    anyhow::ensure!(!value.is_empty(), "argument binding `{name}` has no value");
    Ok(())
}

impl FromStr for ArgumentBinding {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, value) = value
            .split_once('=')
            .ok_or_else(|| anyhow!("argument binding must have the form NAME=VALUE"))?;
        validate_argument_binding(name, value)?;
        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }
}

struct ComposeObjectResolver<'a> {
    transaction: &'a josh_core::cache::Transaction,
    input: gix_hash::ObjectId,
    arguments: BTreeMap<String, gix_hash::ObjectId>,
}

impl ComposeObjectResolver<'_> {
    fn parent(
        &self,
        commit_id: gix_hash::ObjectId,
        parent_number: usize,
        binding: &str,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let commit = josh_core::objects::CommitData::read(self.transaction.odb(), commit_id)
            .with_context(|| {
                format!("argument binding `{binding}` reached unreadable commit {commit_id}")
            })?;
        commit.parent_ids().nth(parent_number - 1).ok_or_else(|| {
            anyhow!(
                "argument binding `{binding}` has no parent {parent_number} at commit {commit_id}"
            )
        })
    }
}

impl ObjectResolver for ComposeObjectResolver<'_> {
    fn resolve(
        &self,
        kind: ObjectKind,
        revision: &str,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>> {
        let suffix_start = revision
            .find(|c| c == '^' || c == '~')
            .unwrap_or(revision.len());
        let (name, mut suffixes) = revision.split_at(suffix_start);
        let binding = if name.is_empty() { "input" } else { name };
        let mut commit_id = if name.is_empty() {
            self.input
        } else if let Some(commit_id) = self.arguments.get(name) {
            *commit_id
        } else {
            return Ok(None);
        };

        while !suffixes.is_empty() {
            let operator = suffixes.as_bytes()[0] as char;
            suffixes = &suffixes[1..];
            let digit_count = suffixes.bytes().take_while(u8::is_ascii_digit).count();
            let digits = &suffixes[..digit_count];
            suffixes = &suffixes[digit_count..];
            let count = if digits.is_empty() {
                1
            } else {
                digits.parse::<usize>().context("invalid ancestry count")?
            };

            match operator {
                '^' if count == 0 => {}
                '^' => commit_id = self.parent(commit_id, count, binding)?,
                '~' => {
                    for _ in 0..count {
                        commit_id = self.parent(commit_id, 1, binding)?;
                    }
                }
                _ => unreachable!("parser only passes validated ancestry suffixes"),
            }
        }

        match kind {
            ObjectKind::Commit => Ok(Some(commit_id)),
            ObjectKind::Tree => {
                let commit =
                    josh_core::objects::CommitData::read(self.transaction.odb(), commit_id)
                        .with_context(|| {
                            format!(
                                "argument binding `{binding}` reached unreadable commit {commit_id}"
                            )
                        })?;
                Ok(Some(commit.tree_id()?))
            }
        }
    }
}

fn resolve_argument_bindings(
    transaction: &josh_core::cache::Transaction,
    bindings: &[ArgumentBinding],
) -> anyhow::Result<BTreeMap<String, gix_hash::ObjectId>> {
    let mut arguments = BTreeMap::new();
    for binding in bindings {
        validate_argument_binding(&binding.name, &binding.value)?;
        anyhow::ensure!(
            !arguments.contains_key(&binding.name),
            "duplicate argument binding `{}`",
            binding.name
        );
        let object_id = transaction
            .rev_parse(&binding.value)
            .with_context(|| {
                format!(
                    "failed to resolve argument binding `{}` from revspec `{}`",
                    binding.name, binding.value
                )
            })?
            .ok_or_else(|| {
                anyhow!(
                    "argument binding `{}` has invalid or missing revspec `{}`",
                    binding.name,
                    binding.value
                )
            })?;
        let commit_id = josh_core::objects::peel_to_commit(transaction.odb(), object_id)
            .with_context(|| {
                format!(
                    "argument binding `{}` revspec `{}` does not resolve to a commit",
                    binding.name, binding.value
                )
            })?;
        arguments.insert(binding.name.clone(), commit_id);
    }
    Ok(arguments)
}

/// Resolve, parse, and apply one compose input.
///
/// Returns the workspace tree OID and canonical filter tree name.
pub fn prepare_workspace(
    transaction: &josh_core::cache::Transaction,
    filter_spec: &str,
    input_ref: &str,
    bindings: &[ArgumentBinding],
) -> anyhow::Result<(gix_hash::ObjectId, String)> {
    let filter_spec = filter_spec.trim();
    let source_commit = josh_core::git::resolve_snapshot_input(transaction, input_ref)
        .with_context(|| format!("failed to resolve input ref: {input_ref:?}"))?;
    let arguments = resolve_argument_bindings(transaction, bindings)?;
    let resolver = ComposeObjectResolver {
        transaction,
        input: source_commit,
        arguments,
    };
    let parsed_filter = josh_filter::parse_with_resolver(filter_spec, &resolver)
        .with_context(|| format!("failed to parse filter: {filter_spec:?}"))?;
    let source_tree = josh_core::objects::CommitData::read(transaction.odb(), source_commit)
        .context("compose input is not a commit")?
        .tree_id()?;
    let parser = |definition: &str| josh_filter::parse_with_resolver(definition, &resolver);
    let user_filter = josh_core::filter::resolve_repository_filters(
        transaction,
        parsed_filter,
        source_tree,
        &parser,
    )
    .context("failed to resolve repository-backed compose filters")?;
    let filter = josh_core::filter::Filter::new().squash().chain(user_filter);

    let filtered_commit = josh_core::filter_commit(transaction, filter, source_commit)
        .context("failed to apply filter")?;
    let ws_tree = josh_core::objects::CommitData::read(transaction.odb(), filtered_commit)
        .context("filtered result is not a commit")?
        .tree_id()?;
    let safe_name = josh_core::filter::as_tree(transaction, filter)
        .context("failed to compute filter id")?
        .to_string();

    Ok((ws_tree, safe_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn oid(value: char) -> gix_hash::ObjectId {
        value.to_string().repeat(40).parse().unwrap()
    }

    fn signature() -> gix::actor::Signature {
        gix::actor::Signature {
            name: "compose".into(),
            email: "compose@example.com".into(),
            time: gix::date::Time {
                seconds: 0,
                offset: 0,
            },
        }
    }

    fn empty_tree(repo: &gix::Repository) -> gix_hash::ObjectId {
        gix::objs::Write::write(
            &repo.objects,
            &gix::objs::Tree {
                entries: Vec::new(),
            },
        )
        .unwrap()
    }

    fn build_tree(repo: &gix::Repository, files: &[(&str, &str)]) -> gix_hash::ObjectId {
        let mut builder = repo.edit_tree(empty_tree(repo)).unwrap();
        for (path, content) in files {
            let blob = josh_core::objects::write_blob(&repo.objects, content.as_bytes()).unwrap();
            builder
                .upsert(*path, gix::objs::tree::EntryKind::Blob, blob)
                .unwrap();
        }
        builder.write().unwrap().detach()
    }

    fn commit(
        repo: &gix::Repository,
        tree: gix_hash::ObjectId,
        parents: &[gix_hash::ObjectId],
    ) -> gix_hash::ObjectId {
        let signature = signature();
        josh_core::objects::write_commit(
            &repo.objects,
            tree,
            parents,
            &signature,
            &signature,
            "test",
        )
        .unwrap()
    }

    fn transaction(repo: &gix::Repository) -> josh_core::cache::Transaction {
        josh_core::cache::TransactionContext::new(
            repo.path(),
            Arc::new(josh_core::cache::CacheStack::new()),
        )
        .open()
        .unwrap()
    }

    fn write_branch(repo: &gix::Repository, name: &str, target: gix_hash::ObjectId) {
        let path = repo.path().join("refs/heads").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("{target}\n")).unwrap();
    }

    fn write_tag(repo: &gix::Repository, name: &str, target: gix_hash::ObjectId) {
        let bytes = format!(
            "object {target}\ntype commit\ntag {name}\ntagger Compose <compose@example.com> 0 +0000\n\nannotated"
        );
        let tag =
            gix::objs::Write::write_buf(&repo.objects, gix::object::Kind::Tag, bytes.as_bytes())
                .unwrap();
        let path = repo.path().join("refs/tags").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("{tag}\n")).unwrap();
    }
    #[test]
    fn binding_syntax_rejects_invalid_reserved_and_empty_values() {
        assert!("baseline=main".parse::<ArgumentBinding>().is_ok());
        for binding in [
            "missing-separator",
            "=main",
            "9baseline=main",
            "base.line=main",
            "input=main",
            "baseline=",
        ] {
            assert!(
                binding.parse::<ArgumentBinding>().is_err(),
                "{binding} should be rejected"
            );
        }
    }

    #[test]
    fn resolver_walks_merge_ancestry_and_projects_trees() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).unwrap();
        let root = commit(&repo, oid('a'), &[]);
        let first = commit(&repo, oid('b'), &[root]);
        let second = commit(&repo, oid('c'), &[root]);
        let merge = commit(&repo, oid('d'), &[first, second]);
        let transaction = transaction(&repo);
        let resolver = ComposeObjectResolver {
            transaction: &transaction,
            input: merge,
            arguments: BTreeMap::from([("baseline".to_owned(), second)]),
        };

        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "").unwrap(),
            Some(merge)
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "^").unwrap(),
            Some(first)
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "^1").unwrap(),
            Some(first)
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "^2").unwrap(),
            Some(second)
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "~2").unwrap(),
            Some(root)
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "^0").unwrap(),
            Some(merge)
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Tree, "^2").unwrap(),
            Some(oid('c'))
        );
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "baseline^").unwrap(),
            Some(root)
        );
        assert_eq!(resolver.resolve(ObjectKind::Commit, "unset").unwrap(), None);
        let error = resolver
            .resolve(ObjectKind::Commit, "^3")
            .unwrap_err()
            .to_string();
        assert!(error.contains("input"));
        assert!(error.contains(&merge.to_string()));
    }

    #[test]
    fn resolver_reads_ephemeral_input_parents_from_transaction_odb() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).unwrap();
        let head = commit(&repo, oid('a'), &[]);
        let transaction = transaction(&repo);
        let signature = signature();
        let snapshot = josh_core::objects::write_commit(
            transaction.odb(),
            oid('b'),
            &[head],
            &signature,
            &signature,
            "snapshot",
        )
        .unwrap();
        assert_eq!(transaction.rev_parse(&snapshot.to_string()).unwrap(), None);

        let resolver = ComposeObjectResolver {
            transaction: &transaction,
            input: snapshot,
            arguments: BTreeMap::new(),
        };
        assert_eq!(
            resolver.resolve(ObjectKind::Commit, "^").unwrap(),
            Some(head)
        );
    }

    #[test]
    fn arguments_resolve_full_revspecs_and_reject_duplicates_or_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).unwrap();
        let root = commit(&repo, oid('a'), &[]);
        let tip = commit(&repo, oid('b'), &[root]);
        write_branch(&repo, "main", tip);
        let transaction = transaction(&repo);
        write_tag(&repo, "v1", tip);

        let resolved =
            resolve_argument_bindings(&transaction, &["baseline=main~1".parse().unwrap()]).unwrap();
        assert_eq!(resolved["baseline"], root);

        let tagged =
            resolve_argument_bindings(&transaction, &["tagged=v1".parse().unwrap()]).unwrap();
        assert_eq!(tagged["tagged"], tip);

        let duplicate = resolve_argument_bindings(
            &transaction,
            &[
                "baseline=main".parse().unwrap(),
                "baseline=main~1".parse().unwrap(),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(duplicate.contains("duplicate argument binding"));

        for revspec in ["missing", "main..main"] {
            let error = resolve_argument_bindings(
                &transaction,
                &[format!("baseline={revspec}").parse().unwrap()],
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(revspec));
        }
    }
    #[test]
    fn preparation_resolves_outer_and_stored_expressions() {
        if !josh_filter::experimental_features_enabled() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).unwrap();
        let parent_tree = build_tree(&repo, &[("selected", "parent")]);
        let parent = commit(&repo, parent_tree, &[]);
        let input_tree = build_tree(
            &repo,
            &[
                ("selected", "current"),
                ("defs/context.josh", ":$.={#baseline|#^}\n"),
            ],
        );
        let input = commit(&repo, input_tree, &[parent]);
        let other_tree = build_tree(&repo, &[("selected", "other")]);
        let other = commit(&repo, other_tree, &[]);
        let transaction = transaction(&repo);

        let (outer_default, _) =
            prepare_workspace(&transaction, ":$.={#baseline|#^}", &input.to_string(), &[]).unwrap();
        assert_eq!(outer_default, parent_tree);
        let (outer_explicit, _) = prepare_workspace(
            &transaction,
            ":$.={#baseline|#^}",
            &input.to_string(),
            &[format!("baseline={other}").parse().unwrap()],
        )
        .unwrap();
        assert_eq!(outer_explicit, other_tree);

        let (stored_default, _) =
            prepare_workspace(&transaction, ":+defs/context", &input.to_string(), &[]).unwrap();
        assert_eq!(
            josh_core::filter::tree::get_blob(
                &transaction,
                transaction.odb(),
                stored_default,
                std::path::Path::new("selected"),
            ),
            "parent"
        );
        assert_eq!(
            josh_core::filter::tree::get_blob(
                &transaction,
                transaction.odb(),
                stored_default,
                std::path::Path::new("defs/context.josh"),
            ),
            ":$.={#baseline|#^}\n"
        );
    }
}
