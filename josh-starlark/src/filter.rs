use allocative::Allocative;
use josh_filter::Filter;
use starlark::{
    environment::{MethodsBuilder, MethodsStatic},
    starlark_module, starlark_simple_value,
    values::{NoSerialize, ProvidesStaticType, StarlarkValue, StringValue},
};
use std::fmt::{self, Display};
use std::path::PathBuf;

/// Opaque Filter type for Starlark
/// We wrap Filter in a newtype that implements the required traits
#[derive(Debug, Clone, Copy, ProvidesStaticType, NoSerialize)]
pub struct StarlarkFilter {
    pub filter: Filter,
}

// Implement Allocative manually since Filter doesn't implement it
// Filter is just a wrapper around git2::Oid which is Copy and small
impl Allocative for StarlarkFilter {
    fn visit<'a, 'b: 'a>(&self, _visitor: &'a mut allocative::Visitor<'b>) {
        // Filter contains only a git2::Oid which is Copy and doesn't need visiting
    }
}

starlark_simple_value!(StarlarkFilter);

impl Display for StarlarkFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Filter({})", self.filter.id())
    }
}

impl<'v> StarlarkValue<'v> for StarlarkFilter {
    type Canonical = Self;

    const TYPE: &'static str = "Filter";

    fn get_type_starlark_repr() -> starlark::typing::Ty {
        starlark::typing::Ty::starlark_value::<Self>()
    }

    fn get_methods() -> Option<&'static starlark::environment::Methods> {
        static RES: MethodsStatic = MethodsStatic::new();
        RES.methods(filter_methods)
    }
}

#[starlark_module]
fn filter_methods(builder: &mut MethodsBuilder) {
    // Builder methods that return Filter
    fn chain(this: &StarlarkFilter, other: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.chain(*other))
    }
    fn nop(this: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.nop())
    }
    fn empty(this: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.empty())
    }
    fn linear(this: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.linear())
    }
    fn file(this: &StarlarkFilter, path: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.file(path))
    }
    fn rename(
        this: &StarlarkFilter,
        dst: StringValue,
        src: StringValue,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.rename(dst, src))
    }
    fn subdir(this: &StarlarkFilter, path: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.subdir(path))
    }
    fn prefix(this: &StarlarkFilter, path: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.prefix(path))
    }
    fn stored(this: &StarlarkFilter, path: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.stored(path))
    }
    fn pattern(this: &StarlarkFilter, pattern: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.pattern(pattern))
    }
    fn workspace(this: &StarlarkFilter, path: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.workspace(path))
    }
    fn author(
        this: &StarlarkFilter,
        name: StringValue,
        email: StringValue,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.author(name, email))
    }
    fn committer(
        this: &StarlarkFilter,
        name: StringValue,
        email: StringValue,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.committer(name, email))
    }
    fn prune_trivial_merge(this: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.prune_trivial_merge())
    }
    fn unsign(this: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.unsign())
    }
    fn message(this: &StarlarkFilter, message: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.message(message))
    }
    fn hook(this: &StarlarkFilter, hook: StringValue) -> anyhow::Result<StarlarkFilter> {
        Ok(this.hook(hook))
    }
    fn with_meta(
        this: &StarlarkFilter,
        key: StringValue,
        value: StringValue,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.with_meta(key, value))
    }
    fn is_nop(this: &StarlarkFilter) -> anyhow::Result<bool> {
        Ok(this.is_nop())
    }
    fn peel(this: &StarlarkFilter) -> anyhow::Result<StarlarkFilter> {
        Ok(this.peel())
    }
}

impl StarlarkFilter {
    /// Create a new Filter
    pub fn new() -> Self {
        Self {
            filter: Filter::new(),
        }
    }

    /// Chain a filter
    pub fn chain(&self, other: StarlarkFilter) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.chain(other.filter),
        }
    }

    /// No-op filter
    pub fn nop(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.nop(),
        }
    }

    /// Check if filter is nop
    pub fn is_nop(&self) -> bool {
        self.filter.is_nop()
    }

    /// Create an empty filter
    pub fn empty(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.empty(),
        }
    }

    /// Linear history filter
    pub fn linear(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.linear(),
        }
    }

    /// File filter
    pub fn file(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.file(PathBuf::from(path.as_str())),
        }
    }

    /// Rename filter
    pub fn rename(&self, dst: StringValue, src: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self
                .filter
                .rename(PathBuf::from(dst.as_str()), PathBuf::from(src.as_str())),
        }
    }

    /// Subdir filter
    pub fn subdir(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.subdir(PathBuf::from(path.as_str())),
        }
    }

    /// Prefix filter
    pub fn prefix(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.prefix(PathBuf::from(path.as_str())),
        }
    }

    /// Stored filter
    pub fn stored(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.stored(PathBuf::from(path.as_str())),
        }
    }

    /// Pattern filter
    pub fn pattern(&self, pattern: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.pattern(pattern.as_str()),
        }
    }

    /// Workspace filter
    pub fn workspace(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.workspace(PathBuf::from(path.as_str())),
        }
    }

    /// Author filter
    pub fn author(&self, name: StringValue, email: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.author(name.as_str(), email.as_str()),
        }
    }

    /// Committer filter
    pub fn committer(&self, name: StringValue, email: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.committer(name.as_str(), email.as_str()),
        }
    }

    /// Prune trivial merge filter
    pub fn prune_trivial_merge(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.prune_trivial_merge(),
        }
    }

    /// Unsign filter
    pub fn unsign(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.unsign(),
        }
    }

    /// Message filter
    pub fn message(&self, message: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.message(message.as_str()),
        }
    }

    /// Hook filter
    pub fn hook(&self, hook: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.hook(hook.as_str()),
        }
    }

    /// With metadata
    pub fn with_meta(&self, key: StringValue, value: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.with_meta(key.as_str(), value.as_str()),
        }
    }

    /// Get metadata
    pub fn get_meta(&self, key: StringValue) -> Option<String> {
        self.filter.get_meta(key.as_str())
    }

    /// Peel metadata
    pub fn peel(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.peel(),
        }
    }
}
