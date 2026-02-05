mod generic;

pub use generic::GenericRemote;

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
    url: impl AsRef<str>,
    trunk_ref: impl AsRef<str>,
) -> anyhow::Result<Box<dyn ChangeVendor>> {
    match vendor {
        Vendor::Generic => Ok(Box::new(GenericRemote {
            repo_path: repo_path.as_ref().into(),
            url: url.as_ref().into(),
            trunk_ref: trunk_ref.as_ref().into(),
        })),
    }
}
