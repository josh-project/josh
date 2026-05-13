pub mod filter;
pub mod flang;
pub mod hash;
pub mod op;
pub mod opt;
pub mod persist;

pub use filter::{Filter, compose};
pub use flang::parse;
pub use flang::{as_file, pretty, spec};
pub use op::LinkMode;
pub use op::{LazyRef, Op, RevMatch};

static EXPERIMENTAL_FEATURES: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var("JOSH_EXPERIMENTAL_FEATURES").as_deref() == Ok("1"));

pub fn experimental_features_enabled() -> bool {
    *EXPERIMENTAL_FEATURES
}

pub fn check_experimental_features_enabled(feature: &str) -> anyhow::Result<()> {
    if experimental_features_enabled() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{feature} requires JOSH_EXPERIMENTAL_FEATURES=1"
        ))
    }
}
