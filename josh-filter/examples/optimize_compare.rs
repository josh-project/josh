//! Rough one-shot timing of the trusted optimizer vs the experimental egg
//! optimizer, on a filter shaped like the `ultrawide_pin` benchmark.
//!
//! This is NOT a criterion benchmark — it builds one wide compose-with-pin
//! filter (≈ `N_FILES` files, nested paths, a `.pin(compose(..))` on top, exactly
//! like `josh-core/benches/ultrawide_pin.rs`), then runs each optimizer a single
//! time and prints the wall-clock duration and output size. The goal is a rough
//! payoff signal, not a statistically rigorous measurement.
//!
//! Two correctness notes for interpreting the numbers:
//!
//! * **Built raw, not via the `Filter` builder.** `Filter::chain`/`compose` call
//!   `opt::optimize` while constructing, so a builder-built filter is already
//!   optimized and re-optimizing would just hit opt's memo cache. We assemble the
//!   `Op` tree with `to_filter` directly so both optimizers see genuinely
//!   un-reduced input — the work opt actually grinds on in `per_rev_filter`.
//!
//! * **Two distinct filters, both cold.** `opt::optimize` memoizes by input OID.
//!   Feeding both optimizers the *same* filter would let the second one reuse the
//!   first's cached result. Instead each optimizer gets its own structurally
//!   identical filter (only the leaf filenames differ, so the decomposition opt
//!   performs is identical), keeping both measurements on a cold cache. This also
//!   means egg's reported time honestly includes its equivalence gate's own cold
//!   `opt::optimize` call on the input.
//!
//! Run release for representative numbers:
//!
//! ```sh
//! cargo run --release --example optimize_compare -p josh-filter
//! ```
use std::path::PathBuf;
use std::time::Instant;

use josh_filter::Filter;
use josh_filter::eggopt::{egg_candidate, equivalent};
use josh_filter::op::Op;
use josh_filter::opt;
use josh_filter::persist::{to_filter, to_op};

/// Number of files in the wide compose, matching the spirit of the benchmark's
/// `N_FILES` (debug 50 / release 500). Bumped to 2000 to make the optimization
/// cost visible in a single-shot timing.
const N_FILES: usize = 2000;

/// Fraction of files held back by the layered `:pin`, mirroring the benchmark's
/// ~25 % hold probability.
const PIN_FRACTION: usize = 4;

/// Build the raw (un-optimized) `wide.pin(compose(pinned))` filter.
///
/// `tag` is folded into every leaf filename so two calls with different tags
/// produce structurally identical but OID-distinct filters (see the module docs).
fn build_wide_pin(tag: &str) -> Filter {
    let files = file_compose(tag, 0..N_FILES);
    let pinned = file_compose(tag, 0..N_FILES / PIN_FRACTION);
    // Chain[ Compose(files), Pin(Compose(pinned)) ] — built with `to_filter` so
    // no optimization runs during construction.
    to_filter(Op::Chain(vec![files, to_filter(Op::Pin(pinned))]))
}

/// A raw `Compose` of `File(dst, dst)` filters for the given indices, with
/// 3-level-nested paths so opt's `step` decomposes them into `Subdir`/`Prefix`
/// chains (the structure that stresses `common_pre`/`prefix_sort`).
fn file_compose(tag: &str, indices: impl Iterator<Item = usize>) -> Filter {
    let files: Vec<Filter> = indices
        .map(|i| {
            // Deterministic pseudo-spread of files across shared subfolders, so
            // the compose has overlapping prefixes for opt to factor.
            let a = (i * 37) % 17;
            let b = (i * 53) % 13;
            let c = i % 7;
            let path = PathBuf::from(format!("d{a}/e{b}/f{c}/file_{i}_{tag}"));
            to_filter(Op::File(path.clone(), path))
        })
        .collect();
    to_filter(Op::Compose(files))
}

/// Count internal `Op` nodes — a proxy for "how reduced is the result".
fn node_count(f: Filter) -> usize {
    fn count(op: &Op) -> usize {
        1 + match op {
            Op::Compose(v) | Op::Chain(v) => v.iter().map(|c| count(&to_op(*c))).sum(),
            Op::Subtract(a, b) => count(&to_op(*a)) + count(&to_op(*b)),
            Op::Exclude(b) | Op::Pin(b) => count(&to_op(*b)),
            _ => 0,
        }
    }
    count(&to_op(f))
}

fn ms(elapsed: std::time::Duration) -> f64 {
    elapsed.as_secs_f64() * 1000.0
}

fn main() {
    let f_opt = build_wide_pin("opt");
    let f_egg = build_wide_pin("egg");
    let in_nodes = node_count(f_opt);

    // opt (cold).
    let t = Instant::now();
    let optimized = opt::optimize(f_opt);
    let opt_time = t.elapsed();

    // egg pipeline, ungated (cold) — reveals egg's raw reduction even when the
    // gate would reject it.
    let t = Instant::now();
    let candidate = egg_candidate(f_egg).expect("wide-pin filter is representable");
    let egg_pipe_time = t.elapsed();
    let cand_nodes = node_count(candidate);
    let cand_reduced = candidate != f_egg;

    // The equivalence gate: equivalent(input, candidate) runs opt on both. Timed
    // separately so the pipeline cost and the gate cost are visible on their own.
    let t = Instant::now();
    let gate_ok = equivalent(f_egg, candidate);
    let gate_time = t.elapsed();
    let egg_final = if gate_ok { candidate } else { f_egg };

    println!("optimize_compare: N_FILES = {N_FILES}, input = {in_nodes} nodes");
    println!(
        "  opt::optimize    : {:>9.2} ms  -> {} nodes",
        ms(opt_time),
        node_count(optimized),
    );
    println!(
        "  egg pipeline     : {:>9.2} ms  -> {} nodes  (ungated candidate; {})",
        ms(egg_pipe_time),
        cand_nodes,
        if cand_reduced {
            format!("reduced {in_nodes} -> {cand_nodes}")
        } else {
            "unchanged".to_string()
        },
    );
    println!(
        "  egg gate check   : {:>9.2} ms  -> {}",
        ms(gate_time),
        if gate_ok { "ACCEPT" } else { "REJECT" },
    );
    println!(
        "  egg_optimize eqv : {:>9.2} ms  -> {} nodes  (pipeline + gate; {})",
        ms(egg_pipe_time + gate_time),
        node_count(egg_final),
        if gate_ok { "kept the reduction" } else { "fell back to input" },
    );
}
