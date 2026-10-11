//! Script evaluation, mirroring the old native `josh-starlark/src/evaluate.rs`.

use crate::filter::StarlarkFilter;
use crate::module::filter_module;
use crate::tree::StarlarkTree;
use anyhow::anyhow;
use josh_filter_guest::Filter;
use starlark::{
    environment::{GlobalsBuilder, Module},
    eval::Evaluator,
    syntax::{AstModule, Dialect},
    values::ValueLike,
};

/// Evaluate a starlark script and return the resulting Filter.
///
/// The script must not use josh filter language strings - all filters
/// must be constructed using the Filter builder methods, starting from the
/// pre-set module variable `filter` (the nop filter). The context-filtered
/// tree is available as the pre-set module variable `tree`. An empty script
/// leaves `filter` untouched and therefore yields the nop filter.
pub fn evaluate(script: &str) -> anyhow::Result<Filter> {
    // Parse the starlark script
    let ast = AstModule::parse("script.star", script.to_owned(), &Dialect::Standard)
        .map_err(|e| anyhow!("Failed to parse starlark script: {}", e))?;

    // Create a new module scoped to a temporary heap
    Module::with_temp_heap(|module| {
        // Build globals with our filter module
        let globals = GlobalsBuilder::standard().with(filter_module).build();

        // Add a global "filter" value (nop filter) to the module
        let filter_value = module.heap().alloc(StarlarkFilter::new());
        module.set("filter", filter_value);

        // Add a global "tree" value (the context-filtered tree) to the module
        let tree_value = module.heap().alloc(StarlarkTree::root());
        module.set("tree", tree_value);

        // Create an evaluator
        let mut eval = Evaluator::new(&module);

        // Evaluate the script
        let _result = eval
            .eval_module(ast, &globals)
            .map_err(|e| anyhow!("Failed to evaluate starlark script: {}", e))?;

        // Try to get the filter from the module
        // Look for a variable named "filter"
        let filter_value = module
            .get("filter")
            .ok_or_else(|| anyhow!("Script must define 'filter' variable returning a Filter"))?;

        // Extract the Filter from the StarlarkFilter value
        let filter = filter_value
            .downcast_ref::<StarlarkFilter>()
            .ok_or_else(|| anyhow!("Expected Filter value, got {}", filter_value.get_type()))?;

        Ok(filter.filter)
    })
}
