use glob::PatternToken;

/// The MatchOptions of the `Op::Pattern` arm: full-path glob matching with literal separators
/// and literal leading dots. Also exactly the options under which the component-wise walk of
/// `tree::remove_pattern` is equivalent to a full-path match.
pub const PATTERN_MATCH_OPTIONS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

/// One '/'-separated component of a glob pattern.
#[derive(Clone)]
pub enum PatternComponent {
    /// A component that is exactly `**`: matches zero or more non-dot-leading path components.
    Star2,
    /// Any other component, matched against single entry names.
    Glob(glob::Pattern),
}

/// The payload of `Op::Pattern`: a glob compiled at construction -- use `Op::pattern` -- for
/// the component-wise NFA walk of `tree::remove_pattern`. `PartialEq`/`Eq`/`Hash` are the
/// source pattern string (recovered verbatim by `as_str`), so `Op` can derive them for use as
/// an interning key.
///
/// Splitting at '/' is sound because the glob crate rejects any `**` that is not a full path
/// component, and under `require_literal_separator` a '/' in the path can only ever be matched
/// by a literal '/' in the pattern -- so a full-path match factorizes into per-component
/// matches. Matching a single component with `matches_with` starts with
/// `follows_separator = true`, which applies the `require_literal_leading_dot` rule at the name
/// start exactly as the full-path match does after a '/'.
#[derive(Clone)]
pub struct CompiledPattern {
    /// The whole pattern, compiled as one glob. Used by the legacy full-path fallback.
    pub full: glob::Pattern,
    pub components: Vec<PatternComponent>,
    /// `suffix_all_star[p]`: components `p..` are all `Star2`, so a blob accepted at position `p`
    /// needs no further components.
    pub suffix_all_star: Vec<bool>,
    /// More than 63 components (the u64 state mask limit): use the legacy full-path walk
    /// instead of the NFA walk.
    pub fallback: bool,
}

impl PartialEq for CompiledPattern {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CompiledPattern {}

impl std::hash::Hash for CompiledPattern {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl std::fmt::Debug for CompiledPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompiledPattern({:?})", self.as_str())
    }
}

/// Split a compiled pattern into path components. This is exact by construction: in the parsed
/// token stream a '/' inside a bracket class is a class member, never a `Char('/')` token, so
/// components are simply the runs between `Char('/')` tokens. A bracket class listing '/' (or a
/// range spanning it) needs no special case either: under `require_literal_separator` a class
/// can never match a separator, in the full-path walk and the per-component walk alike.
///
/// `AnyRecursiveSequence` also ends its component: the parser guarantees `**` is a full path
/// component and consumes the separator that follows it, so no `Char('/')` split ever comes
/// after one. Consecutive `**` components are already collapsed into a single token upstream.
fn split_pattern_components(full: &glob::Pattern) -> Vec<Vec<PatternToken>> {
    let mut parts: Vec<Vec<PatternToken>> = vec![vec![]];
    for token in full.tokens() {
        match token {
            PatternToken::Char('/') => parts.push(vec![]),
            PatternToken::AnyRecursiveSequence => {
                debug_assert!(parts.last().unwrap().is_empty());
                parts.last_mut().unwrap().push(token.clone());
                parts.push(vec![]);
            }
            t => parts.last_mut().unwrap().push(t.clone()),
        }
    }
    // Drop the empty component opened after a trailing `**`: the parser consumed any separator
    // that followed it, so the pattern ends with the `**` component itself.
    if parts.len() >= 2
        && parts.last().unwrap().is_empty()
        && parts[parts.len() - 2] == [PatternToken::AnyRecursiveSequence]
    {
        parts.pop();
    }
    parts
}

impl CompiledPattern {
    pub fn compile(pattern: &str) -> Result<Self, glob::PatternError> {
        // Compile the whole pattern first so error behavior is bit-identical to the full-path
        // implementation (this also rejects any `**` that is not a full path component, which is
        // what makes the '/'-split below sound).
        let full = glob::Pattern::new(pattern)?;
        let components: Vec<PatternComponent> = split_pattern_components(&full)
            .into_iter()
            .map(|tokens| {
                if tokens == [PatternToken::AnyRecursiveSequence] {
                    PatternComponent::Star2
                } else {
                    PatternComponent::Glob(glob::Pattern::from_tokens(tokens))
                }
            })
            .collect();
        // NFA states are u64 bitmasks of component positions.
        let fallback = components.len() > 63;
        let k = components.len();
        let mut suffix_all_star = vec![false; k];
        for p in (0..k).rev() {
            suffix_all_star[p] = matches!(components[p], PatternComponent::Star2)
                && (p + 1 == k || suffix_all_star[p + 1]);
        }
        Ok(CompiledPattern {
            full,
            components,
            suffix_all_star,
            fallback,
        })
    }

    /// The source pattern string, verbatim.
    pub fn as_str(&self) -> &str {
        self.full.as_str()
    }

    /// Epsilon closure of a state mask: a `**` at position `p` can match zero components, so
    /// position `p + 1` is active whenever `p` is. A single ascending pass reaches the fixpoint
    /// because additions only propagate upward. Position `k` is never stored: with the pattern
    /// exhausted, nothing deeper can match under `require_literal_separator` (blob acceptance at
    /// the last component is handled directly in the walk).
    pub fn closure(&self, mut state: u64) -> u64 {
        let k = self.components.len();
        for p in 0..k.saturating_sub(1) {
            if state & (1 << p) != 0 && matches!(self.components[p], PatternComponent::Star2) {
                state |= 1 << (p + 1);
            }
        }
        state
    }

    /// Initial state: component 0 active (plus its closure, taken by `remove_pattern`).
    pub fn initial_state() -> u64 {
        1
    }
}
