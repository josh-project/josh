//! The script-facing `filter.*` method surface, mirroring the old native
//! `josh-starlark/src/filter.rs` on top of the guest SDK's handle-based
//! builder. One change: `starlark(path, sub)` is replaced by
//! `wasm(path, args, sub)`.
//!
//! Methods that raised catchable evaluation errors natively (`pattern` with an
//! invalid glob, `treeid`/`wasm` with the experimental gate disabled) now trap
//! in the host import instead, aborting the whole evaluation — the end state
//! (evaluation failure, empty projection) is the same.

use allocative::Allocative;
use josh_filter_guest::Filter;
use starlark::{
    environment::MethodsBuilder,
    starlark_module, starlark_simple_value,
    values::{NoSerialize, ProvidesStaticType, StarlarkValue, StringValue, list::UnpackList},
};
use std::fmt::{self, Display};

/// Opaque Filter type for Starlark
/// We wrap the guest SDK's Filter handle in a newtype that implements the
/// required traits
#[derive(Debug, Clone, Copy, ProvidesStaticType, NoSerialize)]
pub struct StarlarkFilter {
    pub filter: Filter,
}

// Implement Allocative manually since Filter doesn't implement it
// Filter is just a Copy wrapper around a u32 host handle
impl Allocative for StarlarkFilter {
    fn visit<'a, 'b: 'a>(&self, _visitor: &'a mut allocative::Visitor<'b>) {
        // Filter contains only a u32 handle which is Copy and doesn't need visiting
    }
}

starlark_simple_value!(StarlarkFilter);

impl Display for StarlarkFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The native implementation printed the filter's content OID; the
        // guest only holds an opaque handle, which is still deterministic.
        write!(f, "Filter(#{})", self.filter.handle())
    }
}

impl<'v> StarlarkValue<'v> for StarlarkFilter {
    type Canonical = Self;

    const TYPE: &'static str = "Filter";

    fn get_type_starlark_repr() -> starlark::typing::Ty {
        starlark::typing::Ty::starlark_value::<Self>()
    }

    fn get_methods() -> Option<&'static starlark::environment::Methods> {
        starlark::methods_static!(RES = filter_methods);
        Some(RES.methods())
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
    fn insert(
        this: &StarlarkFilter,
        path: StringValue,
        content: StringValue,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.insert(path, content))
    }
    fn treeid(
        this: &StarlarkFilter,
        path: StringValue,
        subfilter: &StarlarkFilter,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.treeid(path, *subfilter))
    }
    fn wasm(
        this: &StarlarkFilter,
        path: StringValue,
        args: UnpackList<String>,
        subfilter: &StarlarkFilter,
    ) -> anyhow::Result<StarlarkFilter> {
        Ok(this.wasm(path, &args.items, *subfilter))
    }
}

impl StarlarkFilter {
    /// Create a new Filter
    pub fn new() -> Self {
        Self {
            filter: josh_filter_guest::nop(),
        }
    }

    /// Chain a filter
    pub fn chain(&self, other: StarlarkFilter) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.chain(other.filter),
        }
    }

    /// No-op filter — returns self unchanged (NOT a reset to nop)
    pub fn nop(&self) -> StarlarkFilter {
        *self
    }

    /// Check if filter is nop
    pub fn is_nop(&self) -> bool {
        self.filter.is_nop()
    }

    /// Create an empty filter — discards the receiver
    pub fn empty(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: josh_filter_guest::empty(),
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
            filter: self.filter.file(path.as_str()),
        }
    }

    /// Rename filter — the DESTINATION is the first argument
    pub fn rename(&self, dst: StringValue, src: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.rename(dst.as_str(), src.as_str()),
        }
    }

    /// Subdir filter
    pub fn subdir(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.subdir(path.as_str()),
        }
    }

    /// Prefix filter
    pub fn prefix(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.prefix(path.as_str()),
        }
    }

    /// Stored filter
    pub fn stored(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.stored(path.as_str()),
        }
    }

    /// Pattern filter — an invalid glob traps in the host
    pub fn pattern(&self, pattern: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.pattern(pattern.as_str()),
        }
    }

    /// Workspace filter
    pub fn workspace(&self, path: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.workspace(path.as_str()),
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

    /// Insert a blob at path with the given content
    pub fn insert(&self, path: StringValue, content: StringValue) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.insert(path.as_str(), content.as_str()),
        }
    }

    /// Create a blob at path containing the tree OID of subfilter applied to the input
    pub fn treeid(&self, path: StringValue, subfilter: StarlarkFilter) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.treeid(path.as_str(), subfilter.filter),
        }
    }

    /// A nested wasm filter op (replaces the old `starlark(path, sub)`)
    pub fn wasm(
        &self,
        path: StringValue,
        args: &[String],
        subfilter: StarlarkFilter,
    ) -> StarlarkFilter {
        let args: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
        StarlarkFilter {
            filter: self.filter.wasm(path.as_str(), &args, subfilter.filter),
        }
    }

    /// Peel metadata
    pub fn peel(&self) -> StarlarkFilter {
        StarlarkFilter {
            filter: self.filter.peel(),
        }
    }
}
