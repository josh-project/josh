use crate::filter::StarlarkFilter;
use starlark::{
    environment::GlobalsBuilder,
    starlark_module,
    values::{Value, ValueLike},
};

#[starlark_module]
pub fn filter_module(builder: &mut GlobalsBuilder) {
    /// Compose multiple filters together
    /// Creates a filter that overlays the output of filters sequentially
    fn compose<'v>(
        filters: Value<'v>,
        heap: &'v starlark::values::Heap,
    ) -> anyhow::Result<StarlarkFilter> {
        // Get iterator from the value
        let mut iter = filters
            .iterate(heap)
            .map_err(|e| anyhow::anyhow!("Failed to iterate over filters: {}", e))?;

        // Convert to Vec<Filter>
        let mut filter_vec = Vec::new();

        while let Some(item) = iter.next() {
            let starlark_filter = item.downcast_ref::<StarlarkFilter>().ok_or_else(|| {
                anyhow::anyhow!("Expected Filter in compose list, got {}", item.get_type())
            })?;
            filter_vec.push(starlark_filter.filter);
        }

        // Call the Rust compose function
        let composed = josh_filter::compose(&filter_vec);
        Ok(StarlarkFilter { filter: composed })
    }

    // All methods are available directly on Filter instances via the builder API
    // A global "filter" value (nop filter) is available in scripts
    // e.g., filter.subdir("src").prefix("lib")
}
