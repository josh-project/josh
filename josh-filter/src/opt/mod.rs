/*
 * Filter optimization and transformation functions.
 * All those functions convert filters from one equivalent representation into another.
 */

use crate::filter::Filter;
use josh_memodb::PassthroughHasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::sync::LazyLock;

mod flatten;
mod invert;
mod paths;
mod prefix_sort;
mod simplify;
mod step;
mod structure;

pub use flatten::flatten;
pub use invert::invert;
pub use simplify::simplify;

use self::flatten::flatten_full;
use self::step::step;

type FilterHashMap = HashMap<Filter, Filter, BuildHasherDefault<PassthroughHasher>>;
type FilterSet = HashSet<Filter, BuildHasherDefault<PassthroughHasher>>;
type InvertHashMap = HashMap<Filter, Option<Filter>, BuildHasherDefault<PassthroughHasher>>;

static OPTIMIZED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
static MINIMIZED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
static INVERTED: LazyLock<std::sync::Mutex<InvertHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
static SIMPLIFIED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
static FLATTENED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
// Separate cache for the `full` (skip-disabled) flatten: the two produce different results for
// the same filter, so they must not share a memo.
static FLATTENED_FULL: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));

/*
 * Attempt to create an alternative representation of a filter AST that is most
 * suitable for fast evaluation and cache reuse.
 */
pub fn optimize(filter: Filter) -> Filter {
    optimize_impl(filter, false)
}

/*
 * Like `optimize`, but flattens fully (`flatten_full`): distributes even the trailing Compose
 * that `optimize` leaves alone as futile, letting `step` cancel more. For callers that want the
 * maximally-collapsed representation and are not on a hot path (e.g. pretty printing).
 */
pub fn minimize(filter: Filter) -> Filter {
    optimize_impl(filter, true)
}

fn optimize_impl(filter: Filter, minimize: bool) -> Filter {
    let cache = if minimize { &MINIMIZED } else { &OPTIMIZED };
    if let Some(f) = cache.lock().unwrap().get(&filter) {
        return *f;
    }
    let original = filter;

    let mut filter = if minimize {
        flatten_full(filter)
    } else {
        flatten(filter)
    };
    let result = loop {
        let pretty = simplify(filter);
        let optimized = iterate(filter);
        filter = simplify(optimized);

        if filter == pretty {
            break iterate(filter);
        }
    };

    cache.lock().unwrap().insert(original, result);
    result
}

/*
 * Apply optimization steps to a filter until it converges (no rules apply anymore)
 */
fn iterate(filter: Filter) -> Filter {
    let mut filter = filter;
    for _i in 0..1000 {
        let optimized = step(filter);
        if filter == optimized {
            break;
        }
        filter = optimized;
    }
    filter
}
