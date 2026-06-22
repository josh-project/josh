//! Tests for the structural-paths spike (`spike_paths`).
//!
//! Each test maps to one de-risking question (see `spike_paths.rs` module docs and
//! `EGG_OPTIMIZER_POC.md`). They assert on the extracted `RecExpr<PathJosh>`
//! directly — NOT through the `Filter` round-trip — keeping the spike isolated from
//! the main optimizer.

use egg::RecExpr;
use josh_filter::eggopt::spike_paths::{PathJosh, run_spike, spike_reachable};

/// Saturate + extract (`AstSize`) the parsed `input`.
fn run(input: &str) -> RecExpr<PathJosh> {
    let expr: RecExpr<PathJosh> = input.parse().expect("parsed spike input");
    run_spike(&expr)
}

/// Whether `pattern` is reachable in the saturated e-graph of `input`.
fn reachable(input: &str, pattern: &str) -> bool {
    let expr: RecExpr<PathJosh> = input.parse().expect("parsed spike input");
    spike_reachable(&expr, pattern)
}

#[test]
fn subdir_decompose_fires() {
    // Q1 (mechanism): Subdir("a/b") decomposes; the decomposed Chain2 form is
    // reachable even though AstSize extracts the whole path (see next test).
    let input = "(subdir (pathcons a (pathcons b pathnil)))";
    assert!(
        reachable(
            input,
            "(chain2 (subdir (pathcons a pathnil)) (subdir (pathcons b pathnil)))"
        ),
        "subdir-decompose should fire, producing a Chain2 of single-component Subdirs"
    );
}

#[test]
fn astsize_picks_whole_path() {
    // Q2 (decompose ⇄ whole coexist, AstSize non-destructive): the decomposed
    // Chain2(Subdir(a), Subdir(b)) is ~9 nodes vs ~6 for the whole path, so AstSize
    // extracts the whole path. The decomposed form still coexists (reachable). This
    // is the rollback-shaped tension made concrete: extraction does NOT commit to
    // the decomposed form, so structural paths are non-destructive.
    let input = "(subdir (pathcons a (pathcons b pathnil)))";
    assert_eq!(
        run(input).to_string(),
        input,
        "AstSize should pick the whole path over the decomposed chain"
    );
    assert!(
        reachable(input, "(chain2 (subdir (pathcons a pathnil)) ?rest)"),
        "the decomposed form should coexist (be reachable)"
    );
}

#[test]
fn subdir_merge_wins() {
    // E4 path-join: Chain2(Subdir(a), Subdir(b)) -> Subdir(a/b). The merged whole
    // path (~6 nodes) is smaller than the chain (~9), so AstSize picks merged — the
    // mirror of astsize_picks_whole_path from the chain direction. Also confirms
    // path-append saturates to a flat PathCons spine.
    let input = "(chain2 (subdir (pathcons a pathnil)) (subdir (pathcons b pathnil)))";
    assert_eq!(
        run(input).to_string(),
        "(subdir (pathcons a (pathcons b pathnil)))",
        "subdir-merge + path-append should join the two single-component paths"
    );
}

#[test]
fn common_pre_factor_wins() {
    // Q3 — the open question: does common_pre factoring win under AstSize? Input is
    // already decomposed: both Compose members are Chains sharing the single
    // component `a`. The factored form shares Subdir(a) once (~4 nodes) vs twice
    // (~8) unfactored, so AstSize should pick factored. A failure is the actionable
    // finding: structural paths alone do not reclaim factoring.
    let input = "(cons (chain2 (subdir (pathcons a pathnil)) x) \
              (chain2 (subdir (pathcons a pathnil)) y))";
    assert_eq!(
        run(input).to_string(),
        "(chain2 (subdir (pathcons a pathnil)) (cons x y))",
        "if this fails, structural paths alone don't reclaim factoring under AstSize; \
         a cost tweak or directional rules would be needed"
    );
}

#[test]
fn cancel_same_path() {
    // Q4: Prefix and Subdir of the SAME structural path cancel to nop (pure pattern
    // — ?p unifies the two equal PathCons spines by e-class identity).
    let input = "(chain2 (prefix (pathcons a pathnil)) (subdir (pathcons a pathnil)))";
    assert_eq!(run(input).to_string(), "nop");
}

#[test]
fn conflict_different_path_same_depth() {
    // Q4: Prefix(a)·Subdir(b), same depth (1), different path -> empty via the
    // PathConflict applier (walks two PathCons spines, compares depth + disequality).
    let input = "(chain2 (prefix (pathcons a pathnil)) (subdir (pathcons b pathnil)))";
    assert_eq!(run(input).to_string(), "empty");
}
