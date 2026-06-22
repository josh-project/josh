use crate::eggopt::appliers::{PrefixSubdirConflict, SubtractComposeDiff};
use crate::eggopt::lang::{Josh, JoshAnalysis};
use egg::{EGraph, Id, Rewrite, Subst, Var, rewrite};

/// The rewrites this POC runs.
///
/// All gated on the same semantic bar — the output must produce an equivalent
/// tree and history, checked by [`crate::eggopt::equivalent`] using the trusted
/// optimizer as a sufficient-but-not-necessary oracle. They capture the *spirit*
/// of `opt.rs`, not its exact mechanism.
///
/// `Compose` is a cons-list (`Cons`/`Nil`), so the rules that were arity-limited
/// under `Box<[Id]>` (dedup, empty-removal, flatten, pluck, absorb) now match a
/// list of **any length** at a fixed arity, and the two variadic rules that were
/// impossible as patterns (dedup at any position, membership-gated absorb) become a
/// 2-arity pattern plus an element-set [`JoshAnalysis`] condition. `Chain` stays
/// `Box<[Id]>` (it is ordered; cons order non-determinism is fine for `Compose`, a
/// set, but wrong for `Chain`).
///
/// * compose flatten (2 rules): a nested compose splices into the outer one by
///   append-via-cons, mirroring `opt`'s recursive `flatten`. `compose-flatten`
///   peels the head element out of a nested list per fire
///   (`(cons (cons ?a ?ta) ?tail) => (cons ?a (cons ?ta ?tail))`), and
///   `compose-flatten-nil` drops a `Nil` head — the base case, produced when a
///   nested list is peeled to nothing (e.g. a `Compose[Compose[x], t]` whose
///   nested list collapses to `Nil`). To fixpoint this flattens nesting at any
///   depth; the base case is distinct from `compose-drop-empty`, which matches the
///   `empty` *atom* (`Op::Empty`), not `Nil` (an empty *list*).
/// * cancel-prefix-subdir / prefix-subdir-conflict: unchanged (2-element chains).
/// * compose empty-removal + dedup: `(cons empty ?t) => ?t` (any position, pure
///   pattern) and `(cons ?x ?t) => ?t` when `?t`'s element-set contains `?x` (dedup
///   at any position, incl. non-consecutive — `opt`'s consecutive-only `Vec::dedup`
///   misses that). Singleton/empty composes collapse at `rebuild`, not as a rule.
/// * exclude / pin identity: unchanged.
/// * subtract identity / Message-Message: unchanged.
/// * subtract pluck (2 pure patterns, any arity/position): pluck-head removes the
///   element when it is the list head; pluck-deeper pushes the subtract one step
///   down the spine. To fixpoint they remove an element from anywhere.
/// * subtract absorb (1 pattern + condition): `(subtract ?x ?l) => empty` when
///   `?l`'s set contains `?x`.
/// * subtract-compose-diff ([`SubtractComposeDiff`] applier): the full bidirectional
///   `Subtract(Compose A, Compose B)` set-difference — variadic, so still an
///   applier (cons-aware); the single-element cases are the pluck/absorb rules.
///
/// Now present (structural paths promoted): path decomposition
/// (`subdir-decompose`/`prefix-decompose`, opt E6/E7). Decompose is honest and
/// non-destructive (both forms coexist; `AstSize` picks), and it lets egg MATCH
/// `opt` on the Prefix/Subdir conflict cases that need it (see `eggopt_cancel`).
///
/// `common_pre` (shared head) and `common_post` (shared tail) factoring are NOT
/// rules here. As rules each failed: `common_post` as a matched `Applier` on
/// `(cons ?h ?tail)` was O(N²) (the LHS matches ~every cons cell of a large
/// compose and re-factors each suffix); `common_pre` as a pairwise rule could not
/// factor three or more chains (its intermediate is larger than the input, so
/// `AstSize` rejects the partial factoring and run-to-fixpoint never converges).
/// Both are instead targeted passes applied between saturation rounds — see
/// `eggopt::egg_candidate`, [`crate::eggopt::appliers::factor_all_common_pre`],
/// and [`crate::eggopt::appliers::factor_all_common_post`]. Together they let egg
/// match `opt`'s shared-prefix/shared-suffix factoring on the corpus (see
/// `corpus_gaps`). Still absent: the variadic `distribute`/absorb inverses.
pub(crate) fn rules() -> Vec<Rewrite<Josh, JoshAnalysis>> {
    vec![
        rewrite!("cancel-prefix-subdir";
            "(chaincons (prefix ?p) (chaincons (subdir ?p) ?rest))" => "?rest"),
        // Prefix/Subdir conflict (same depth, different path -> empty): custom
        // applier for the disequality + depth guard. See [`PrefixSubdirConflict`].
        rewrite!("prefix-subdir-conflict";
            "(chaincons (prefix ?a) (chaincons (subdir ?b) ?rest))" =>
            { PrefixSubdirConflict::new() }),
        // E5 — an empty path (PathNil) is identity for Prefix/Subdir.
        rewrite!("subdir-empty"; "(subdir pathnil)" => "nop"),
        rewrite!("prefix-empty"; "(prefix pathnil)" => "nop"),
        // E6 Subdir decompose — FORWARD (opt step has no .rev()). The two-deep
        // guard matches only >=2-component paths, so a single component
        // (pathcons h pathnil) is the base case and does not oscillate against
        // subdir-empty. Decompose is a node-count LOSS, so AstSize keeps the whole
        // path unless a downstream rule (common-pre-factor) makes the decomposed
        // form pay off. The second element is wrapped in its own ChainCons (a
        // proper 2-element chain) — a bare leaf as the ChainCons tail would conflate
        // Chain's spine with the element and make rebuild bail.
        rewrite!("subdir-decompose";
            "(subdir (pathcons ?h (pathcons ?h2 ?t)))"
            => "(chaincons (subdir (pathcons ?h pathnil)) (chaincons (subdir (pathcons ?h2 ?t)) chainnil))"),
        // E7 Prefix decompose — REVERSED (opt step does .rev()): the LAST
        // component becomes the FIRST chain element. Asymmetric with subdir. Same
        // ChainCons-tail wrap as subdir-decompose.
        rewrite!("prefix-decompose";
            "(prefix (pathcons ?h (pathcons ?h2 ?t)))"
            => "(chaincons (prefix (pathcons ?h2 ?t)) (chaincons (prefix (pathcons ?h pathnil)) chainnil))"),
        // Chain normalize: flatten nested chains (associative, order-preserving)
        // so decompose's nested output meets cancel/conflict/common_pre at any
        // position; drop a ChainNil head (the flatten base case); eliminate Nop
        // (the chain identity); and propagate Empty (any empty in a chain empties
        // the whole chain, mirroring opt step).
        rewrite!("chain-flatten";
            "(chaincons (chaincons ?x ?y) ?z)" => "(chaincons ?x (chaincons ?y ?z))"),
        rewrite!("chain-drop-chainnil"; "(chaincons chainnil ?t)" => "?t"),
        rewrite!("chain-nop-l"; "(chaincons nop ?t)" => "?t"),
        rewrite!("chain-empty-l"; "(chaincons empty ?t)" => "empty"),
        rewrite!("chain-empty-r"; "(chaincons ?h empty)" => "empty"),
        // common_pre factoring is a targeted pass, not a rule — see the rule-set
        // doc comment above and `eggopt::egg_candidate`. As a pairwise pattern rule
        // it could not factor three or more chains sharing a head (its intermediate
        // is larger than the input, so `AstSize` rejected the partial factoring);
        // `factor_all_common_pre` factors all N shared-head chains in one O(N) walk.
        // common_post factoring is a targeted pass for the same reason — see
        // `eggopt::egg_candidate`.
        // Compose flatten (E1): a nested compose splices into the outer one by
        // append-via-cons, mirroring opt's recursive flatten. compose-flatten peels
        // one head element out of a nested list per fire; compose-flatten-nil drops
        // a Nil head — the base case, produced when a nested list is peeled to
        // nothing (a Nil head is an empty sub-compose, the identity). The base case
        // is needed: peeling a singleton nested list Cons(x, Nil) leaves a Nil head,
        // and without this rebuild would emit an Empty element where the empty
        // compose should simply vanish. Run to fixpoint this flattens any depth.
        rewrite!("compose-flatten";
            "(cons (cons ?a ?ta) ?tail)" =>
            "(cons ?a (cons ?ta ?tail))"),
        rewrite!("compose-flatten-nil";
            "(cons nil ?tail)" => "?tail"),
        // Compose empty-removal (any position): empty is the identity of Compose
        // (parallel merge), so a leading empty element is dropped; run to fixpoint
        // it drops empties anywhere. Singleton/empty composes collapse at rebuild.
        rewrite!("compose-drop-empty";
            "(cons empty ?tail)" => "?tail"),
        // Compose dedup (any position): drop a head element that also appears later
        // in the list. Run to fixpoint this dedups the whole list — including
        // non-consecutive duplicates, the case opt's consecutive-only Vec::dedup
        // misses. Compose is a set, so keeping any occurrence is the canonical tree.
        rewrite!("compose-dedup";
            "(cons ?x ?tail)" => "?tail" if contains("?x", "?tail")),
        // Exclude / Pin identity (unchanged): nop/empty are the opaque leaf atoms.
        rewrite!("exclude-nop"; "(exclude nop)" => "empty"),
        rewrite!("exclude-empty"; "(exclude empty)" => "nop"),
        rewrite!("pin-empty"; "(pin empty)" => "nop"),
        // Subtract identity / annihilator algebra (unchanged, pure patterns).
        rewrite!("subtract-self"; "(subtract ?x ?x)" => "empty"),
        rewrite!("subtract-empty-l"; "(subtract empty ?x)" => "empty"),
        rewrite!("subtract-nop-r"; "(subtract ?x nop)" => "empty"),
        rewrite!("subtract-empty-r"; "(subtract ?x empty)" => "?x"),
        // Any two Message filters have an empty tree difference: a Message only
        // rewrites commit metadata, never the tree. Pure pattern (Message is
        // structural); the two ?m bindings are distinct, so this also covers
        // identical messages (which subtract-self reaches first).
        rewrite!("subtract-message-message";
            "(subtract (message ?m1) (message ?m2))" => "empty"),
        // Subtract pluck (any arity/position): remove an element from a cons-list.
        // pluck-head fires when the element is the list head; pluck-deeper pushes
        // the subtract one step down the spine. To fixpoint, removes it from
        // anywhere; if absent, the subtract bottoms out at the empty list.
        rewrite!("pluck-head";
            "(subtract (cons ?x ?tail) ?x)" => "?tail"),
        rewrite!("pluck-deeper";
            "(subtract (cons ?h ?tail) ?x)" =>
            "(cons ?h (subtract ?tail ?x))"),
        // Subtract absorb: an element contained in a cons-list subtracts to empty.
        rewrite!("absorb-into-list";
            "(subtract ?x ?list)" => "empty" if contains("?x", "?list")),
        // Subtract bidirectional set-difference over two composes — still variadic,
        // so a custom applier (now cons-aware). See [`SubtractComposeDiff`].
        rewrite!("subtract-compose-diff";
            "(subtract ?a ?b)" => { SubtractComposeDiff::new() }),
    ]
}

/// Condition for `compose-dedup` and `absorb-into-list`: the cons-list bound to
/// `list` has an element-set (the [`JoshAnalysis`] annotation) containing the
/// canonical representative of `elem`. A closure of this shape implements
/// [`egg::Condition`].
fn contains(
    elem: &str,
    list: &str,
) -> impl Fn(&mut EGraph<Josh, JoshAnalysis>, Id, &Subst) -> bool {
    let elem_var = elem.parse::<Var>().expect("elem var");
    let list_var = list.parse::<Var>().expect("list var");
    move |egraph, _eclass, subst| {
        let e = *subst.get(elem_var).expect("bound elem");
        let l = *subst.get(list_var).expect("bound list");
        egraph[l].data.contains(&egraph.find(e))
    }
}
