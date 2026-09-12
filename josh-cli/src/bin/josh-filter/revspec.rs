use anyhow::{Context, anyhow};

pub(super) fn resolve(
    transaction: &josh_core::cache::Transaction,
    revspecs: &[String],
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let specs = revspecs
        .iter()
        .map(|spec| {
            let parsed =
                gix::refspec::parse(spec.as_str().into(), gix::refspec::parse::Operation::Fetch)
                    .with_context(|| format!("invalid revspec: {spec}"))?
                    .to_owned();
            if parsed.to_ref().source().is_none() || parsed.to_ref().destination().is_none() {
                anyhow::bail!("revspec must map a source to a destination: {spec}");
            }
            Ok(parsed)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut available = Vec::new();
    transaction.for_each_ref_prefixed("", |name, oid| {
        available.push((name.to_owned(), oid));
        Ok(())
    })?;

    let matches =
        gix::refspec::MatchGroup::from_fetch_specs(specs.iter().map(gix::refspec::RefSpec::to_ref))
            .match_lhs(
                available
                    .iter()
                    .map(|(name, oid)| gix::refspec::match_group::Item {
                        full_ref_name: name.as_str().into(),
                        target: oid,
                        object: None,
                    }),
            )
            .mappings;

    if matches.is_empty() {
        anyhow::bail!("revspec did not match any refs");
    }

    let mut destinations = std::collections::BTreeSet::new();
    matches
        .into_iter()
        .map(|mapping| {
            let source_index = mapping
                .item_index
                .ok_or_else(|| anyhow!("revspec sources must name refs"))?;
            let destination = mapping
                .rhs
                .ok_or_else(|| anyhow!("revspec must map every source to a destination"))?;
            let destination = std::str::from_utf8(destination.as_ref())?.to_owned();
            gix::refs::FullName::try_from(destination.as_str())
                .with_context(|| format!("invalid destination ref: {destination}"))?;
            if !destinations.insert(destination.clone()) {
                anyhow::bail!("multiple revspecs map to destination ref: {destination}");
            }
            let oid =
                josh_core::objects::peel_to_commit(transaction.odb(), available[source_index].1)?;
            Ok((destination, oid))
        })
        .collect()
}
