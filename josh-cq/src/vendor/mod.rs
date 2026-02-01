mod generic;

pub use generic::GenericRemote;
use std::collections::BTreeMap;

use crate::change::ChangeGraph;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Vendor {
    #[default]
    Generic,
}

pub trait ChangeVendor {
    fn list_changes(&self) -> anyhow::Result<ChangeGraph>;
}

pub fn make_vendor(
    vendor: Vendor,
    repo_path: impl AsRef<std::path::Path>,
    refs: &BTreeMap<String, git2::Oid>,
    trunk: (String, git2::Oid),
) -> anyhow::Result<Box<dyn ChangeVendor>> {
    match vendor {
        Vendor::Generic => {
            let refs = refs.clone();

            Ok(Box::new(GenericRemote {
                repo_path: repo_path.as_ref().into(),
                refs,
                trunk,
            }))
        }
    }
}
