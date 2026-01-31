use crate::filter::StarlarkFilter;
use crate::module::filter_module;
use crate::tree::StarlarkTree;
use josh_filter::Filter;
use starlark::{
    environment::{GlobalsBuilder, Module},
    eval::Evaluator,
    syntax::{AstModule, Dialect},
    values::ValueLike,
};
use std::sync::{Arc, Mutex};

/// Evaluate a starlark script and return the resulting Filter
///
/// The script should define a function or variable that returns a Filter.
/// The script must not use josh filter language strings - all filters
/// must be constructed using the Filter builder methods.
///
/// The `tree_oid` parameter is made available as a global variable named "tree"
/// in the Starlark script, allowing access to the git tree via methods.
pub fn evaluate(
    script: &str,
    tree_oid: git2::Oid,
    repo: Arc<Mutex<git2::Repository>>,
) -> anyhow::Result<Filter> {
    // Parse the starlark script
    let ast = AstModule::parse("script.star", script.to_owned(), &Dialect::Standard)
        .map_err(|e| anyhow::anyhow!("Failed to parse starlark script: {}", e))?;

    // Create a new module
    let module = Module::new();

    // Build globals with our filter module
    let globals = GlobalsBuilder::standard().with(filter_module).build();

    // Add a global "filter" value (nop filter) to the module
    let filter_value = module.heap().alloc(StarlarkFilter::new());
    module.set("filter", filter_value);

    // Add a global "tree" value (the git tree) to the module
    let tree_value = module.heap().alloc(StarlarkTree::new(tree_oid, repo));
    module.set("tree", tree_value);

    // Create an evaluator
    let mut eval = Evaluator::new(&module);

    // Evaluate the script
    let _result = eval
        .eval_module(ast, &globals)
        .map_err(|e| anyhow::anyhow!("Failed to evaluate starlark script: {}", e))?;

    // Try to get the filter from the module
    // Look for a variable named "filter"
    let filter_value = module.get("filter").ok_or_else(|| {
        anyhow::anyhow!("Script must define 'filter' variable returning a Filter")
    })?;

    // Extract the Filter from the StarlarkFilter value
    let filter = filter_value
        .downcast_ref::<StarlarkFilter>()
        .ok_or_else(|| anyhow::anyhow!("Expected Filter value, got {}", filter_value.get_type()))?;

    Ok(filter.filter)
}
