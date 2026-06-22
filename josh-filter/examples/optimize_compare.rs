//! Rough scaling sweep of the trusted optimizer vs the experimental egg
//! optimizer, on the pin-legalized filter shape from `ultrawide_pin`.
//!
//! This is NOT a criterion benchmark — for each size `N` it builds the
//! pin-legalized shape that `per_rev_filter` feeds to `opt` (a `Compose` of `N`
//! wide-chains, each with `compose(pinned)` as its trailing element), then runs
//! each optimizer's *pipeline* once and prints the wall-clock duration and output
//! size. The point is the **scaling curve**: opt's `flatten` distributes that
//! trailing compose into an O(N·|pinned|) tree that `step`'s `common_pre` then
//! collapses again — the round trip commit `b01736b1b4` patched (unsoundly) — so
//! opt is superlinear here. egg no longer carries the `distribute` rule (dropped
//! with the chain-over-compose factoring family), so it never expands the trailing
//! compose and scales gently on this shape — at the cost of not building opt's
//! factored form. This example confirms egg avoids the pathology.
//!
//! Three correctness notes for interpreting the numbers:
//!
//! * **Built raw, not via the `Filter` builder.** `Filter::chain`/`compose` call
//!   `opt::optimize` while constructing, so a builder-built filter is already
//!   optimized. We assemble the `Op` tree with `to_filter` directly so both
//!   optimizers see genuinely un-reduced input.
//!
//! * **Distinct filters per (optimizer, size), all cold.** `opt::optimize`
//!   memoizes by input OID, so the tag encodes both the optimizer and the size,
//!   giving every measurement its own OID-distinct filter (same shape/size, so
//!   the work is identical) and keeping every run cold. The tag is also folded
//!   into every path component so sub-problems don't share across sizes.
//!
//! * **Pipeline time only, not the gate.** `egg_optimize`'s equivalence gate
//!   runs `opt` internally, so its total time would just mirror opt's curve and
//!   hide egg's own scaling. We time `egg_candidate` (the ungated pipeline)
//!   instead. The gate's soundness-vs-completeness is a separate question.
//!
//! Before the timed sweep a small in-process warmup is discarded, so the first
//! measured row is not inflated by one-time egg/EGraph and allocator setup.
//!
//! Run release for representative numbers:
//!
//! ```sh
//! cargo run --release --example optimize_compare -p josh-filter
//! cargo run --release --example optimize_compare -p josh-filter -- --egg-only
//! cargo run --release --example optimize_compare -p josh-filter -- --max-count 5000
//! ```
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use josh_filter::eggopt::egg_candidate;
use josh_filter::op::Op;
use josh_filter::persist::{to_filter, to_op};
use josh_filter::{Filter, opt};

#[derive(Parser)]
#[command(about = "Scaling sweep of opt vs egg optimizer on wide-pin shape")]
struct Args {
    /// Skip opt::optimize; only run the egg pipeline.
    #[arg(long)]
    egg_only: bool,

    /// Maximum N; sweep picks 3 points (start, ×5, end) per power-of-10 decade
    /// from 10 up to this value.
    #[arg(long, default_value_t = 1000)]
    max_count: usize,
}

/// Fraction of files held back by the layered `:pin`, mirroring the benchmark's
/// ~25 % hold probability.
const PIN_FRACTION: usize = 4;

/// Size used for the discarded in-process warmup. Moderate, so it exercises the
/// `common_post` reduction path without adding much wall-clock before the sweep.
const WARMUP_SIZE: usize = 200;

/// Number of discarded warmup iterations. The first `egg_candidate` call in a
/// process pays one-time costs (egg/EGraph setup, allocator caches, CPU frequency
/// ramp) — ~200 ms vs ~10 ms warm at N=100 — so without a warmup the first measured
/// row reads as a spurious outlier. A few iterations prime these before any row is
/// timed.
const WARMUP_ITERS: usize = 3;

/// Generate sweep sizes: for each decade [10ᵏ, 10ᵏ⁺¹] up to `max_count`
/// emit the start, midpoint (×5), and end, then deduplicate adjacent repeats.
fn sweep_sizes(max_count: usize) -> Vec<usize> {
    let mut sizes = vec![];
    let mut decade = 10;
    while decade < max_count {
        let end = (decade * 10).min(max_count);
        sizes.extend([decade, decade * 5, end]);
        decade *= 10;
    }
    sizes.dedup();
    sizes
}

/// Build the raw (un-optimized) pin-legalized shape for `n_files`: a `Compose` of
/// `N` chains `Chain[file_i, Compose(pinned)]` — `compose(pinned)` as the bare
/// trailing element of each wide-chain, the structure whose `flatten`-distribution
/// is the O(N·|pinned|) blowup. `tag` makes the filter OID-distinct per
/// (optimizer, size) so caches stay cold (see the module docs).
fn build_wide_pin(n_files: usize, tag: &str) -> Filter {
    let pinned = file_compose(tag, 0..n_files / PIN_FRACTION);
    let elements: Vec<Filter> = (0..n_files)
        .map(|i| to_filter(Op::Chain(vec![single_file(tag, i), pinned])))
        .collect();
    to_filter(Op::Compose(elements))
}

/// A raw `Compose` of `File(dst, dst)` filters for the given indices.
fn file_compose(tag: &str, indices: impl Iterator<Item = usize>) -> Filter {
    to_filter(Op::Compose(indices.map(|i| single_file(tag, i)).collect()))
}

/// One raw `File(dst, dst)` filter with a 3-level-nested path, so the optimizers
/// decompose it into `Subdir`/`Prefix` chains (the structure that stresses
/// `common_pre`/`prefix_sort`).
fn single_file(tag: &str, i: usize) -> Filter {
    // Deterministic pseudo-spread across shared subfolders, so a compose has
    // overlapping prefixes to factor.
    let a = (i * 37) % 17;
    let b = (i * 53) % 13;
    let c = i % 7;
    let path = PathBuf::from(format!("d{a}_{tag}/e{b}_{tag}/f{c}_{tag}/file_{i}"));
    to_filter(Op::File(path.clone(), path))
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
    let args = Args::parse();
    let sizes = sweep_sizes(args.max_count);

    // Header
    if args.egg_only {
        println!("{:>6}  {:>10}  {:>9}", "N", "egg (ms)", "egg nodes");
    } else {
        println!(
            "{:>6}  {:>10}  {:>10}  {:>10}  {:>9}  {:>9}",
            "N", "opt (ms)", "egg (ms)", "opt/egg", "opt nodes", "egg nodes",
        );
    }

    // Discarded in-process warmup: prime egg/EGraph init, allocator caches, and CPU
    // frequency ramp so the first *measured* row isn't a one-time-cost outlier. The
    // warmup tags are distinct from the per-size `opt{n}`/`egg{n}` tags, so opt's
    // by-OID memo is not primed for any measurement.
    for i in 0..WARMUP_ITERS {
        let f = build_wide_pin(WARMUP_SIZE, &format!("warmup{i}"));
        if !args.egg_only {
            let _ = opt::optimize(f);
        }
        let _ = egg_candidate(f);
    }

    for &n in &sizes {
        // eprintln!("build wide pin");
        let f_egg = build_wide_pin(n, &format!("egg{n}"));

        // eprintln!("count in nodes");
        let in_nodes = node_count(f_egg);

        // eprintln!("firing egg");
        let t = Instant::now();
        let candidate = egg_candidate(f_egg).expect("wide-pin filter is representable");
        let egg_ms = ms(t.elapsed());

        // eprintln!("counting nodes");
        let egg_nodes = node_count(candidate);

        if args.egg_only {
            println!(
                "{:>6}  {:>10.2}  {:>9}  (in: {})",
                n, egg_ms, egg_nodes, in_nodes,
            );
            continue;
        }

        // Distinct tags per optimizer → distinct OIDs → cold caches at each size.
        let f_opt = build_wide_pin(n, &format!("opt{n}"));

        let t = Instant::now();
        let optimized = opt::optimize(f_opt);
        let opt_ms = ms(t.elapsed());
        let opt_nodes = node_count(optimized);

        let ratio = if egg_ms > 0.0 {
            opt_ms / egg_ms
        } else {
            f64::NAN
        };
        println!(
            "{:>6}  {:>10.2}  {:>10.2}  {:>10.2}  {:>9}  {:>9}  (in: {})",
            n, opt_ms, egg_ms, ratio, opt_nodes, egg_nodes, in_nodes,
        );
    }
}
