use anyhow::Context;
use regex_syntax::hir::{Class, Hir, HirKind};
use std::collections::BTreeSet;

use super::fold_char;

const MAX_EXACT: usize = 7;
const MAX_SET: usize = 20;

type FoldedString = Vec<u8>;
type StringSet = BTreeSet<FoldedString>;

/// A conservative Boolean query over the trigram index. Every text matched by the regular
/// expression satisfies this query; texts satisfying it still require regex verification.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Query {
    All,
    None,
    Trigram([u8; 3]),
    And(Vec<Query>),
    Or(Vec<Query>),
}

impl Query {
    fn and(self, other: Query) -> Query {
        Self::combine(self, other, true)
    }

    fn or(self, other: Query) -> Query {
        Self::combine(self, other, false)
    }

    fn combine(left: Query, right: Query, is_and: bool) -> Query {
        if left == right {
            return left;
        }
        match (&left, &right, is_and) {
            (Query::None, _, true) | (_, Query::None, true) => return Query::None,
            (Query::All, _, false) | (_, Query::All, false) => return Query::All,
            (Query::All, _, true) => return right,
            (_, Query::All, true) => return left,
            (Query::None, _, false) => return right,
            (_, Query::None, false) => return left,
            _ => {}
        }

        let mut children = vec![];
        let mut append = |query| match (is_and, query) {
            (true, Query::And(sub)) | (false, Query::Or(sub)) => children.extend(sub),
            (_, query) => children.push(query),
        };
        append(left);
        append(right);
        children.sort();
        children.dedup();

        // Absorption: q AND (q OR r) = q, and q OR (q AND r) = q.
        let remove: Vec<bool> = children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let opposite = match (is_and, child) {
                    (true, Query::Or(sub)) | (false, Query::And(sub)) => Some(sub),
                    _ => None,
                };
                opposite.is_some_and(|sub| {
                    children.iter().enumerate().any(|(other_index, atom)| {
                        index != other_index && sub.binary_search(atom).is_ok()
                    })
                })
            })
            .collect();
        let mut index = 0;
        children.retain(|_| {
            let keep = !remove[index];
            index += 1;
            keep
        });

        if children.len() == 1 {
            children.pop().unwrap()
        } else if is_and {
            Query::And(children)
        } else {
            Query::Or(children)
        }
    }

    fn and_trigrams(self, strings: &StringSet) -> Query {
        if strings.is_empty() {
            return Query::None;
        }
        if strings.iter().any(|s| s.len() < 3) {
            return self;
        }

        let mut alternatives: Vec<_> = strings
            .iter()
            .map(|string| {
                let trigrams: BTreeSet<[u8; 3]> = string
                    .windows(3)
                    .map(|bytes| bytes.try_into().unwrap())
                    .collect();
                let mut terms: Vec<_> = trigrams.into_iter().map(Query::Trigram).collect();
                if terms.len() == 1 {
                    terms.pop().unwrap()
                } else {
                    Query::And(terms)
                }
            })
            .collect();
        alternatives.sort();
        alternatives.dedup();
        let alternatives = if alternatives.len() == 1 {
            alternatives.pop().unwrap()
        } else {
            Query::Or(alternatives)
        };
        self.and(alternatives)
    }

    pub(crate) fn trigrams(&self, out: &mut BTreeSet<[u8; 3]>) {
        match self {
            Query::Trigram(trigram) => {
                out.insert(*trigram);
            }
            Query::And(sub) | Query::Or(sub) => {
                for query in sub {
                    query.trigrams(out);
                }
            }
            Query::All | Query::None => {}
        }
    }

    /// Trigrams every match must contain. This deliberately loses the branch-specific
    /// constraints of an OR and is only used by the compatibility `query_roots` API.
    pub(crate) fn required_trigrams(&self) -> BTreeSet<[u8; 3]> {
        match self {
            Query::All | Query::None => BTreeSet::new(),
            Query::Trigram(trigram) => BTreeSet::from([*trigram]),
            Query::And(sub) => sub.iter().flat_map(Query::required_trigrams).collect(),
            Query::Or(sub) => {
                let Some((first, rest)) = sub.split_first() else {
                    return BTreeSet::new();
                };
                let mut required = first.required_trigrams();
                for query in rest {
                    let branch = query.required_trigrams();
                    required.retain(|trigram| branch.contains(trigram));
                }
                required
            }
        }
    }

    pub(crate) fn conjunctive_trigrams(&self) -> Option<BTreeSet<[u8; 3]>> {
        match self {
            Query::All => Some(BTreeSet::new()),
            Query::Trigram(trigram) => Some(BTreeSet::from([*trigram])),
            Query::And(sub) => {
                let mut trigrams = BTreeSet::new();
                for query in sub {
                    trigrams.extend(query.conjunctive_trigrams()?);
                }
                Some(trigrams)
            }
            Query::None | Query::Or(_) => None,
        }
    }
}

pub(crate) struct Plan {
    pub(crate) regex: regex::Regex,
    pub(crate) query: Query,
}

impl Plan {
    pub(crate) fn new(pattern: &str) -> anyhow::Result<Self> {
        let regex = regex::Regex::new(pattern).context("invalid search regular expression")?;
        let hir = regex_syntax::Parser::new()
            .parse(pattern)
            .context("invalid search regular expression")?;
        let mut info = analyze(&hir);
        info.simplify(true);
        info.add_exact();
        Ok(Self {
            regex,
            query: info.query,
        })
    }
}

struct Info {
    can_empty: bool,
    exact: Option<StringSet>,
    prefix: StringSet,
    suffix: StringSet,
    query: Query,
}

impl Info {
    fn any_match() -> Self {
        Self {
            can_empty: true,
            exact: None,
            prefix: StringSet::from([vec![]]),
            suffix: StringSet::from([vec![]]),
            query: Query::All,
        }
    }

    fn any_char() -> Self {
        Self {
            can_empty: false,
            exact: None,
            prefix: StringSet::from([vec![]]),
            suffix: StringSet::from([vec![]]),
            query: Query::All,
        }
    }

    fn no_match() -> Self {
        Self {
            can_empty: false,
            exact: Some(StringSet::new()),
            prefix: StringSet::new(),
            suffix: StringSet::new(),
            query: Query::None,
        }
    }

    fn empty_string() -> Self {
        Self {
            can_empty: true,
            exact: Some(StringSet::from([vec![]])),
            prefix: StringSet::new(),
            suffix: StringSet::new(),
            query: Query::All,
        }
    }

    fn exact(strings: StringSet) -> Self {
        Self {
            can_empty: strings.contains(&vec![]),
            exact: Some(strings),
            prefix: StringSet::new(),
            suffix: StringSet::new(),
            query: Query::All,
        }
    }

    fn add_exact(&mut self) {
        if let Some(exact) = &self.exact {
            self.query = std::mem::replace(&mut self.query, Query::All).and_trigrams(exact);
        }
    }

    fn simplify(&mut self, force: bool) {
        let flush = self.exact.as_ref().is_some_and(|exact| {
            exact.len() > MAX_EXACT || (min_len(exact) >= 3 && force) || min_len(exact) >= 4
        });
        if flush {
            self.add_exact();
            for string in self.exact.take().unwrap() {
                if string.len() < 3 {
                    self.prefix.insert(string.clone());
                    self.suffix.insert(string);
                } else {
                    self.prefix.insert(string[..2].to_vec());
                    self.suffix.insert(string[string.len() - 2..].to_vec());
                }
            }
        }

        if self.exact.is_none() {
            self.simplify_set(false);
            self.simplify_set(true);
        }
    }

    fn simplify_set(&mut self, suffix: bool) {
        let mut strings = if suffix {
            std::mem::take(&mut self.suffix)
        } else {
            std::mem::take(&mut self.prefix)
        };
        self.query = std::mem::replace(&mut self.query, Query::All).and_trigrams(&strings);

        let mut length = 3;
        loop {
            strings = strings
                .into_iter()
                .map(|string| {
                    if string.len() < length {
                        string
                    } else if suffix {
                        string[string.len() - length + 1..].to_vec()
                    } else {
                        string[..length - 1].to_vec()
                    }
                })
                .collect();
            if strings.len() <= MAX_SET {
                break;
            }
            length = length.saturating_sub(1);
        }

        let values: Vec<_> = strings.into_iter().collect();
        let reduced = values
            .iter()
            .filter(|candidate| {
                !values.iter().any(|shorter| {
                    shorter.len() < candidate.len()
                        && if suffix {
                            candidate.ends_with(shorter)
                        } else {
                            candidate.starts_with(shorter)
                        }
                })
            })
            .cloned()
            .collect();
        if suffix {
            self.suffix = reduced;
        } else {
            self.prefix = reduced;
        }
    }
}

fn min_len(strings: &StringSet) -> usize {
    strings.iter().map(Vec::len).min().unwrap_or(0)
}

fn cross(left: &StringSet, right: &StringSet) -> StringSet {
    left.iter()
        .flat_map(|left| {
            right.iter().map(move |right| {
                let mut joined = Vec::with_capacity(left.len() + right.len());
                joined.extend_from_slice(left);
                joined.extend_from_slice(right);
                joined
            })
        })
        .collect()
}

fn concat(mut left: Info, mut right: Info) -> Info {
    let query = left.query.and(right.query);
    let mut result = Info {
        can_empty: left.can_empty && right.can_empty,
        exact: None,
        prefix: StringSet::new(),
        suffix: StringSet::new(),
        query,
    };

    match (&left.exact, &right.exact) {
        (Some(left_exact), Some(right_exact)) => {
            result.exact = Some(cross(left_exact, right_exact));
        }
        (Some(left_exact), None) => {
            result.prefix = cross(left_exact, &right.prefix);
            result.suffix = std::mem::take(&mut right.suffix);
            if right.can_empty {
                result.suffix.extend(std::mem::take(&mut left.suffix));
            }
        }
        (None, Some(right_exact)) => {
            result.prefix = std::mem::take(&mut left.prefix);
            if left.can_empty {
                result.prefix.extend(std::mem::take(&mut right.prefix));
            }
            result.suffix = cross(&left.suffix, right_exact);
        }
        (None, None) => {
            result.prefix = std::mem::take(&mut left.prefix);
            if left.can_empty {
                result.prefix.extend(std::mem::take(&mut right.prefix));
            }
            result.suffix = std::mem::take(&mut right.suffix);
            if right.can_empty {
                result.suffix.extend(std::mem::take(&mut left.suffix));
            }
        }
    }

    if left.exact.is_none()
        && right.exact.is_none()
        && left.suffix.len() <= MAX_SET
        && right.prefix.len() <= MAX_SET
        && min_len(&left.suffix) + min_len(&right.prefix) >= 3
    {
        result.query = result
            .query
            .and_trigrams(&cross(&left.suffix, &right.prefix));
    }

    result.simplify(false);
    result
}

fn alternate(mut left: Info, mut right: Info) -> Info {
    let mut result = Info {
        can_empty: left.can_empty || right.can_empty,
        exact: None,
        prefix: StringSet::new(),
        suffix: StringSet::new(),
        query: left.query.clone().or(right.query.clone()),
    };

    match (&left.exact, &right.exact) {
        (Some(left_exact), Some(right_exact)) => {
            let mut exact = left_exact.clone();
            exact.extend(right_exact.iter().cloned());
            result.exact = Some(exact);
        }
        (Some(left_exact), None) => {
            result.prefix = left_exact.clone();
            result.prefix.extend(std::mem::take(&mut right.prefix));
            result.suffix = left_exact.clone();
            result.suffix.extend(std::mem::take(&mut right.suffix));
            left.add_exact();
            result.query = left.query.or(right.query);
        }
        (None, Some(right_exact)) => {
            result.prefix = std::mem::take(&mut left.prefix);
            result.prefix.extend(right_exact.iter().cloned());
            result.suffix = std::mem::take(&mut left.suffix);
            result.suffix.extend(right_exact.iter().cloned());
            right.add_exact();
            result.query = left.query.or(right.query);
        }
        (None, None) => {
            result.prefix = std::mem::take(&mut left.prefix);
            result.prefix.extend(std::mem::take(&mut right.prefix));
            result.suffix = std::mem::take(&mut left.suffix);
            result.suffix.extend(std::mem::take(&mut right.suffix));
        }
    }

    result.simplify(false);
    result
}

fn analyze(hir: &Hir) -> Info {
    let mut info = match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => Info::empty_string(),
        HirKind::Literal(literal) => {
            let Ok(literal) = std::str::from_utf8(&literal.0) else {
                return Info::any_match();
            };
            Info::exact(StringSet::from([literal.chars().map(fold_char).collect()]))
        }
        HirKind::Class(class) => analyze_class(class),
        HirKind::Capture(capture) => return analyze(&capture.sub),
        HirKind::Concat(sub) => {
            return sub.iter().map(analyze).fold(Info::empty_string(), concat);
        }
        HirKind::Alternation(sub) => {
            return sub.iter().map(analyze).fold(Info::no_match(), alternate);
        }
        HirKind::Repetition(repetition) => {
            if repetition.min == 0 {
                if repetition.max == Some(1) {
                    return alternate(analyze(&repetition.sub), Info::empty_string());
                }
                return Info::any_match();
            }
            let mut repeated = analyze(&repetition.sub);
            if let Some(exact) = repeated.exact.take() {
                repeated.prefix = exact.clone();
                repeated.suffix = exact;
            }
            repeated
        }
    };
    info.simplify(false);
    info
}

fn analyze_class(class: &Class) -> Info {
    let mut symbols = BTreeSet::new();
    match class {
        Class::Unicode(class) => {
            for byte in 0_u8..=127 {
                let c = char::from(byte);
                if class
                    .ranges()
                    .iter()
                    .any(|range| range.start() <= c && c <= range.end())
                {
                    symbols.insert(vec![fold_char(c)]);
                }
            }
            if class.ranges().iter().any(|range| range.end() >= '\u{80}') {
                symbols.insert(vec![super::NON_ASCII_GLYPH]);
            }
        }
        Class::Bytes(class) => {
            for range in class.ranges() {
                for byte in range.start()..=range.end() {
                    symbols.insert(vec![if byte.is_ascii() {
                        fold_char(char::from(byte))
                    } else {
                        super::NON_ASCII_GLYPH
                    }]);
                }
            }
        }
    }

    if symbols.is_empty() {
        Info::no_match()
    } else if symbols.len() > 100 {
        Info::any_char()
    } else {
        Info::exact(symbols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigrams(pattern: &str) -> BTreeSet<[u8; 3]> {
        let plan = Plan::new(pattern).unwrap();
        let mut trigrams = BTreeSet::new();
        plan.query.trigrams(&mut trigrams);
        trigrams
    }

    #[test]
    fn literal_and_concat_extract_trigrams() {
        assert_eq!(
            trigrams("Google.*Search"),
            [
                *b"goo", *b"oog", *b"ogl", *b"gle", *b"sea", *b"ear", *b"arc", *b"rch"
            ]
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn alternation_and_classes_keep_branch_constraints() {
        let plan = Plan::new("ab[cd]e").unwrap();
        assert!(matches!(plan.query, Query::Or(_)));
        assert_eq!(
            trigrams("ab[cd]e"),
            [*b"abc", *b"abd", *b"bce", *b"bde"].into_iter().collect()
        );
    }

    #[test]
    fn optional_short_branch_requires_no_trigram() {
        assert_eq!(Plan::new("foo|x").unwrap().query, Query::All);
        assert_eq!(Plan::new("(?:abc)?").unwrap().query, Query::All);
    }

    #[test]
    fn regex_syntax_and_unicode_folding_are_supported() {
        let plan = Plan::new(r"(?i)^naïve\s+[0-9]{2,}$").unwrap();
        assert!(plan.regex.is_match("NAÏVE 42"));
        assert!(trigrams(r"(?i)naïve").contains(&[b'a', crate::NON_ASCII_GLYPH, b'v']));
    }
}
