// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Support for matching file paths against Unix shell style patterns.
//!
//! The `glob` and `glob_with` functions, in concert with the `Paths`
//! type, allow querying the filesystem for all files that match a particular
//! pattern - just like the libc `glob` function (for an example see the `glob`
//! documentation). The methods on the `Pattern` type provide functionality
//! for checking if individual paths match a particular pattern - in a similar
//! manner to the libc `fnmatch` function
//! For consistency across platforms, and for Windows support, this module
//! is implemented entirely in Rust rather than deferring to the libc
//! `glob`/`fnmatch` functions.

#![doc(html_logo_url = "http://www.rust-lang.org/logos/rust-logo-128x128-blk-v2.png",
       html_favicon_url = "http://www.rust-lang.org/favicon.ico",
       html_root_url = "http://doc.rust-lang.org/glob/")]
#![allow(unstable)]

use std::ascii::AsciiExt;
use std::cell::Cell;
use std::{cmp, path};
use std::io::fs::{self, PathExtensions};
use std::path::is_sep;
use std::string::String;
use std::fmt;

use PatternToken::{Char, AnyChar, AnySequence, AnyRecursiveSequence, AnyWithin, AnyExcept};
use CharSpecifier::{SingleChar, CharRange};
use MatchResult::{Match, SubPatternDoesntMatch, EntirePatternDoesntMatch};

/// An iterator that yields Paths from the filesystem that match a particular
/// pattern - see the `glob` function for more details.
pub struct Paths {
    dir_patterns: Vec<Pattern>,
    require_dir: bool,
    options: MatchOptions,
    todo: Vec<(Path,usize)>,
}

/// Return an iterator that produces all the Paths that match the given pattern,
/// which may be absolute or relative to the current working directory.
///
/// This may return an error if the pattern is invalid.
///
/// This method uses the default match options and is equivalent to calling
/// `glob_with(pattern, MatchOptions::new())`. Use `glob_with` directly if you
/// want to use non-default match options.
///
/// # Example
///
/// Consider a directory `/media/pictures` containing only the files `kittens.jpg`,
/// `puppies.jpg` and `hamsters.gif`:
///
/// ```rust
/// use glob::glob;
///
/// for path in glob("/media/pictures/*.jpg").unwrap() {
///     println!("{}", path.display());
/// }
/// ```
///
/// The above code will print:
///
/// ```ignore
/// /media/pictures/kittens.jpg
/// /media/pictures/puppies.jpg
/// ```
///
pub fn glob(pattern: &str) -> Result<Paths, Error> {
    glob_with(pattern, &MatchOptions::new())
}

/// Return an iterator that produces all the Paths that match the given pattern,
/// which may be absolute or relative to the current working directory.
///
/// This may return an error if the pattern is invalid.
///
/// This function accepts Unix shell style patterns as described by `Pattern::new(..)`.
/// The options given are passed through unchanged to `Pattern::matches_with(..)` with
/// the exception that `require_literal_separator` is always set to `true` regardless of the
/// value passed to this function.
///
/// Paths are yielded in alphabetical order.
pub fn glob_with(pattern: &str, options: &MatchOptions) -> Result<Paths, Error> {
    // make sure that the pattern is valid first, else early return with error
    let _compiled = try!(Pattern::new(pattern));

    #[cfg(windows)]
    fn check_windows_verbatim(p: &Path) -> bool { path::windows::is_verbatim(p) }
    #[cfg(not(windows))]
    fn check_windows_verbatim(_: &Path) -> bool { false }

    #[cfg(windows)]
    fn to_scope(p: Path) -> Path {
        use std::os::getcwd;

        if path::windows::is_vol_relative(&p) {
            let mut cwd = getcwd().unwrap();
            cwd.push(p);
            cwd
        } else {
            p
        }
    }
    #[cfg(not(windows))]
    fn to_scope(p: Path) -> Path { p }

    let root = Path::new(pattern).root_path();
    let root_len = root.as_ref().map_or(0us, |p| p.as_vec().len());

    if root.is_some() && check_windows_verbatim(root.as_ref().unwrap()) {
        // FIXME: How do we want to handle verbatim paths? I'm inclined to return nothing,
        // since we can't very well find all UNC shares with a 1-letter server name.
        return Ok(Paths {
            dir_patterns: Vec::new(),
            require_dir: false,
            options: options.clone(),
            todo: Vec::new(),
        });
    }

    let scope = root.map(to_scope).unwrap_or_else(|| Path::new("."));

    let dir_patterns = pattern.slice_from(cmp::min(root_len, pattern.len()))
                       .split_terminator(is_sep)
                       .map(|s| Pattern::new(s).unwrap())
                       .collect::<Vec<Pattern>>();

    let require_dir = pattern.chars().next_back().map(is_sep) == Some(true);

    let mut todo = Vec::new();
    if dir_patterns.len() > 0 {
        // Shouldn't happen, but we're using -1 as a special index.
        assert!(dir_patterns.len() < -1 as usize);

        fill_todo(&mut todo, dir_patterns.as_slice(), 0, &scope, options);
    }

    Ok(Paths {
        dir_patterns: dir_patterns,
        require_dir: require_dir,
        options: options.clone(),
        todo: todo,
    })
}

impl Iterator for Paths {
    type Item = Path;

    fn next(&mut self) -> Option<Path> {
        loop {
            if self.dir_patterns.is_empty() || self.todo.is_empty() {
                return None;
            }

            let (path,idx) = self.todo.pop().unwrap();

            // idx -1: was already checked by fill_todo, maybe path was '.' or
            // '..' that we can't match here because of normalization.
            if idx == -1 as usize {
                if self.require_dir && !path.is_dir() { continue; }
                return Some(path);
            }

            let ref pattern = self.dir_patterns[idx];
            let is_recursive = pattern.is_recursive;
            let is_last = idx == self.dir_patterns.len() - 1;

            // special casing for recursive patterns when globbing
            //   if it's a recursive pattern and it's not the last dir_patterns,
            //   test if it matches the next non-recursive pattern,
            //   if it does, then move to the pattern after the next pattern
            //   otherwise accept the path based on the recursive pattern
            //   and remain on the recursive pattern
            if is_recursive && !is_last {
                // the next non-recursive pattern
                let mut next = idx + 1;

                // collapse consecutive recursive patterns
                while next < self.dir_patterns.len() && self.dir_patterns[next].is_recursive {
                    next += 1;
                }

                // no non-recursive patterns follow the current one
                // so auto-accept all remaining recursive paths
                if next == self.dir_patterns.len() {
                    fill_todo(&mut self.todo, self.dir_patterns.as_slice(),
                              next - 1, &path, &self.options);
                    return Some(path);
                }

                let ref next_pattern = self.dir_patterns[next];
                let is_match = next_pattern.matches_with(match path.filename_str() {
                    // this ugly match needs to go here to avoid a borrowck error
                    None => {
                        // FIXME (#9639): How do we handle non-utf8 filenames? Ignore them for now
                        // Ideally we'd still match them against a *
                        continue;
                    }
                    Some(x) => x
                }, &self.options);

                // determine how to advance
                let (current_idx, next_idx) =
                    if is_match {
                        // accept the pattern after the next non-recursive pattern
                        (next, next + 1)
                    } else {
                        // next pattern still hasn't matched
                        // so stay on this recursive pattern
                        (next - 1, next - 1)
                    };

                if current_idx == self.dir_patterns.len() - 1 {
                    // it is not possible for a pattern to match a directory *AND* its children
                    // so we don't need to check the children

                    if !self.require_dir || path.is_dir() {
                        return Some(path);
                    }
                } else {
                    fill_todo(&mut self.todo, self.dir_patterns.as_slice(),
                              next_idx, &path, &self.options);
                }
            }

            // it's recursive and it's the last pattern
            // automatically match everything else recursively
            else if is_recursive && is_last {
              fill_todo(&mut self.todo, self.dir_patterns.as_slice(),
                        idx, &path, &self.options);
              return Some(path);
            }

            // not recursive, so match normally
            else if pattern.matches_with(match path.filename_str() {
                // this ugly match needs to go here to avoid a borrowck error
                None => {
                    // FIXME (#9639): How do we handle non-utf8 filenames? Ignore them for now
                    // Ideally we'd still match them against a *
                    continue;
                }
                Some(x) => x
            }, &self.options) {
                if idx == self.dir_patterns.len() - 1 {
                    // it is not possible for a pattern to match a directory *AND* its children
                    // so we don't need to check the children

                    if !self.require_dir || path.is_dir() {
                        return Some(path);
                    }
                } else {
                    fill_todo(&mut self.todo, self.dir_patterns.as_slice(),
                              idx + 1, &path, &self.options);
                }
            }
        }
    }

}

fn list_dir_sorted(path: &Path) -> Option<Vec<Path>> {
    match fs::readdir(path) {
        Ok(mut children) => {
            children.sort_by(|p1, p2| p2.filename().cmp(&p1.filename()));
            Some(children.into_iter().collect())
        }
        Err(..) => None
    }
}

/// A pattern parsing error.
pub struct Error {
  /// The approximate character index of where the error occurred.
  pub pos: usize,

  /// A message describing the error.
  pub msg: &'static str,
}

impl fmt::String for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "Pattern syntax error near position {}: {}",
           self.pos, self.msg)
  }
}

impl fmt::Show for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::String::fmt(self, f)
  }
}

/// A compiled Unix shell style pattern.
///
/// `?` matches any single character
///
/// `*` matches any (possibly empty) sequence of characters
///
/// `**` matches the current directory and arbitrary subdirectories. This sequence **must** form a single path
/// component, so both `**a` and `b**` are invalid and will result in an error.
/// A sequence of more than two consecutive `*` characters is also invalid.
///
/// `[...]` matches any character inside the brackets.
/// Character sequences can also specify ranges
/// of characters, as ordered by Unicode, so e.g. `[0-9]` specifies any
/// character between 0 and 9 inclusive. An unclosed bracket is invalid.
///
/// `[!...]` is the negation of `[...]`, i.e. it matches any characters **not**
/// in the brackets.
///
/// The metacharacters `?`, `*`, `[`, `]` can be matched by using brackets
/// (e.g. `[?]`).  When a `]` occurs immediately following `[` or `[!` then
/// it is interpreted as being part of, rather then ending, the character
/// set, so `]` and NOT `]` can be matched by `[]]` and `[!]]` respectively.
/// The `-` character can be specified inside a character sequence pattern by
/// placing it at the start or the end, e.g. `[abc-]`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Pattern {
    original: String,
    tokens: Vec<PatternToken>,
    is_recursive: bool,
}

/// Show the original glob pattern.
impl fmt::String for Pattern {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.original.fmt(f)
  }
}

/// Show the original glob pattern.
impl fmt::Show for Pattern {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    fmt::String::fmt(self, f)
  }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum PatternToken {
    Char(char),
    AnyChar,
    AnySequence,
    AnyRecursiveSequence,
    AnyWithin(Vec<CharSpecifier> ),
    AnyExcept(Vec<CharSpecifier> )
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum CharSpecifier {
    SingleChar(char),
    CharRange(char, char)
}

#[derive(Copy, PartialEq)]
enum MatchResult {
    Match,
    SubPatternDoesntMatch,
    EntirePatternDoesntMatch
}

const ERROR_WILDCARDS: &'static str =
  "wildcards are either regular `*` or recursive `**`";
const ERROR_RECURSIVE_WILDCARDS: &'static str =
  "recursive wildcards must form a single path component";
const ERROR_INVALID_RANGE: &'static str =
  "invalid range pattern";

impl Pattern {
    /// This function compiles Unix shell style patterns.
    ///
    /// An invalid glob pattern will yield an error.
    pub fn new(pattern: &str) -> Result<Pattern, Error> {

        let chars = pattern.chars().collect::<Vec<_>>();
        let mut tokens = Vec::new();
        let mut is_recursive = false;
        let mut i = 0;

        while i < chars.len() {
            match chars[i] {
                '?' => {
                    tokens.push(AnyChar);
                    i += 1;
                }
                '*' => {
                    let old = i;

                    while i < chars.len() && chars[i] == '*' {
                        i += 1;
                    }

                    let count = i - old;

                    if count > 2 {
                        return Err(
                          Error {
                            pos: old + 2,
                            msg: ERROR_WILDCARDS,
                          });
                    } else if count == 2 {
                        // ** can only be an entire path component
                        // i.e. a/**/b is valid, but a**/b or a/**b is not
                        // invalid matches are treated literally
                        let is_valid =
                            // is the beginning of the pattern or begins with '/'
                            if i == 2 || chars[i - count - 1] == '/' {
                                // it ends in a '/'
                                if i < chars.len() && chars[i] == '/' {
                                    i += 1;
                                    true
                                // or the pattern ends here
                                // this enables the existing globbing mechanism
                                } else if i == chars.len() {
                                    true
                                // `**` ends in non-separator
                                } else {
                                    return Err(
                                      Error  {
                                        pos: i,
                                        msg: ERROR_RECURSIVE_WILDCARDS,
                                        });
                                }
                            // `**` begins with non-separator
                            } else {
                                return Err(
                                  Error  {
                                    pos: old - 1,
                                    msg: ERROR_RECURSIVE_WILDCARDS,
                                    });
                            };

                        let tokens_len = tokens.len();

                        if is_valid {
                            // collapse consecutive AnyRecursiveSequence to a single one
                            if !(tokens_len > 1 && tokens[tokens_len - 1] == AnyRecursiveSequence) {
                                is_recursive = true;
                                tokens.push(AnyRecursiveSequence);
                            }
                        }
                    } else {
                        tokens.push(AnySequence);
                    }
                }
                '[' => {

                    if i <= chars.len() - 4 && chars[i + 1] == '!' {
                        match chars.slice_from(i + 3).position_elem(&']') {
                            None => (),
                            Some(j) => {
                                let chars = chars.slice(i + 2, i + 3 + j);
                                let cs = parse_char_specifiers(chars);
                                tokens.push(AnyExcept(cs));
                                i += j + 4;
                                continue;
                            }
                        }
                    }
                    else if i <= chars.len() - 3 && chars[i + 1] != '!' {
                        match chars.slice_from(i + 2).position_elem(&']') {
                            None => (),
                            Some(j) => {
                                let cs = parse_char_specifiers(chars.slice(i + 1, i + 2 + j));
                                tokens.push(AnyWithin(cs));
                                i += j + 3;
                                continue;
                            }
                        }
                    }

                    // if we get here then this is not a valid range pattern
                    return Err(
                      Error  {
                        pos: i,
                        msg: ERROR_INVALID_RANGE,
                    });
                }
                c => {
                    tokens.push(Char(c));
                    i += 1;
                }
            }
        }

        Ok(Pattern {
            tokens: tokens,
            original: pattern.to_string(),
            is_recursive: is_recursive,
        })
    }

    /// Escape metacharacters within the given string by surrounding them in
    /// brackets. The resulting string will, when compiled into a `Pattern`,
    /// match the input string and nothing else.
    pub fn escape(s: &str) -> String {
        let mut escaped = String::new();
        for c in s.chars() {
            match c {
                // note that ! does not need escaping because it is only special inside brackets
                '?' | '*' | '[' | ']' => {
                    escaped.push('[');
                    escaped.push(c);
                    escaped.push(']');
                }
                c => {
                    escaped.push(c);
                }
            }
        }
        escaped
    }

    /// Return if the given `str` matches this `Pattern` using the default
    /// match options (i.e. `MatchOptions::new()`).
    ///
    /// # Example
    ///
    /// ```rust
    /// use glob::Pattern;
    ///
    /// assert!(Pattern::new("c?t").unwrap().matches("cat"));
    /// assert!(Pattern::new("k[!e]tteh").unwrap().matches("kitteh"));
    /// assert!(Pattern::new("d*g").unwrap().matches("doog"));
    /// ```
    pub fn matches(&self, str: &str) -> bool {
        self.matches_with(str, &MatchOptions::new())
    }

    /// Return if the given `Path`, when converted to a `str`, matches this `Pattern`
    /// using the default match options (i.e. `MatchOptions::new()`).
    pub fn matches_path(&self, path: &Path) -> bool {
        // FIXME (#9639): This needs to handle non-utf8 paths
        path.as_str().map_or(false, |s| {
            self.matches(s)
        })
    }

    /// Return if the given `str` matches this `Pattern` using the specified match options.
    pub fn matches_with(&self, str: &str, options: &MatchOptions) -> bool {
        self.matches_from(None, str, 0, options) == Match
    }

    /// Return if the given `Path`, when converted to a `str`, matches this `Pattern`
    /// using the specified match options.
    pub fn matches_path_with(&self, path: &Path, options: &MatchOptions) -> bool {
        // FIXME (#9639): This needs to handle non-utf8 paths
        path.as_str().map_or(false, |s| {
            self.matches_with(s, options)
        })
    }

    /// Access the original glob pattern.
    pub fn as_str<'a>(&'a self) -> &'a str {
      self.original.as_slice()
    }

    fn matches_from(&self,
                    prev_char: Option<char>,
                    mut file: &str,
                    i: usize,
                    options: &MatchOptions) -> MatchResult {

        let prev_char = Cell::new(prev_char);

        let require_literal = |&: c| {
            (options.require_literal_separator && is_sep(c)) ||
            (options.require_literal_leading_dot && c == '.'
             && is_sep(prev_char.get().unwrap_or('/')))
        };

        for (ti, token) in self.tokens.slice_from(i).iter().enumerate() {
            match *token {
                AnySequence | AnyRecursiveSequence => {
                    loop {
                        match self.matches_from(prev_char.get(), file, i + ti + 1, options) {
                            SubPatternDoesntMatch => (), // keep trying
                            m => return m,
                        }

                        let (c, next) = match file.slice_shift_char() {
                            None => return EntirePatternDoesntMatch,
                            Some(pair) => pair
                        };

                        if let AnySequence = *token {
                            if require_literal(c) {
                                return SubPatternDoesntMatch;
                            }
                        }

                        prev_char.set(Some(c));
                        file = next;
                    }
                }
                _ => {
                    let (c, next) = match file.slice_shift_char() {
                        None => return EntirePatternDoesntMatch,
                        Some(pair) => pair
                    };

                    let matches = match *token {
                        AnyChar => {
                            !require_literal(c)
                        }
                        AnyWithin(ref specifiers) => {
                            !require_literal(c) &&
                                in_char_specifiers(specifiers.as_slice(),
                                                   c,
                                                   options)
                        }
                        AnyExcept(ref specifiers) => {
                            !require_literal(c) &&
                                !in_char_specifiers(specifiers.as_slice(),
                                                    c,
                                                    options)
                        }
                        Char(c2) => {
                            chars_eq(c, c2, options.case_sensitive)
                        }
                        AnySequence | AnyRecursiveSequence => {
                            unreachable!()
                        }
                    };
                    if !matches {
                        return SubPatternDoesntMatch;
                    }
                    prev_char.set(Some(c));
                    file = next;
                }
            }
        }

        if file.is_empty() {
            Match
        } else {
            SubPatternDoesntMatch
        }
    }

}

// Fills `todo` with paths under `path` to be matched by `patterns[idx]`,
// special-casing patterns to match `.` and `..`, and avoiding `readdir()`
// calls when there are no metacharacters in the pattern.
fn fill_todo(todo: &mut Vec<(Path, usize)>, patterns: &[Pattern], idx: usize, path: &Path,
             options: &MatchOptions) {
    // convert a pattern that's just many Char(_) to a string
    fn pattern_as_str(pattern: &Pattern) -> Option<String> {
        let mut s = String::new();
        for token in pattern.tokens.iter() {
            match *token {
                Char(c) => s.push(c),
                _ => return None
            }
        }
        return Some(s);
    }

    let add = |&: todo: &mut Vec<_>, next_path: Path| {
        if idx + 1 == patterns.len() {
            // We know it's good, so don't make the iterator match this path
            // against the pattern again. In particular, it can't match
            // . or .. globs since these never show up as path components.
            todo.push((next_path, -1 as usize));
        } else {
            fill_todo(todo, patterns, idx + 1, &next_path, options);
        }
    };

    let pattern = &patterns[idx];

    match pattern_as_str(pattern) {
        Some(s) => {
            // This pattern component doesn't have any metacharacters, so we
            // don't need to read the current directory to know where to
            // continue. So instead of passing control back to the iterator,
            // we can just check for that one entry and potentially recurse
            // right away.
            let special = "." == s.as_slice() || ".." == s.as_slice();
            let next_path = path.join(s.as_slice());
            if (special && path.is_dir()) || (!special && next_path.exists()) {
                add(todo, next_path);
            }
        },
        None => {
            match list_dir_sorted(path) {
                Some(entries) => {
                    todo.extend(entries.into_iter().map(|x|(x, idx)));

                    // Matching the special directory entries . and .. that refer to
                    // the current and parent directory respectively requires that
                    // the pattern has a leading dot, even if the `MatchOptions` field
                    // `require_literal_leading_dot` is not set.
                    if pattern.tokens.len() > 0 && pattern.tokens[0] == Char('.') {
                        for &special in [".", ".."].iter() {
                            if pattern.matches_with(special, options) {
                                add(todo, path.join(special));
                            }
                        }
                    }
                }
                None => {}
            }
        }
    }
}

fn parse_char_specifiers(s: &[char]) -> Vec<CharSpecifier> {
    let mut cs = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if i + 3 <= s.len() && s[i + 1] == '-' {
            cs.push(CharRange(s[i], s[i + 2]));
            i += 3;
        } else {
            cs.push(SingleChar(s[i]));
            i += 1;
        }
    }
    cs
}

fn in_char_specifiers(specifiers: &[CharSpecifier], c: char, options: &MatchOptions) -> bool {

    for &specifier in specifiers.iter() {
        match specifier {
            SingleChar(sc) => {
                if chars_eq(c, sc, options.case_sensitive) {
                    return true;
                }
            }
            CharRange(start, end) => {

                // FIXME: work with non-ascii chars properly (issue #1347)
                if !options.case_sensitive && c.is_ascii() && start.is_ascii() && end.is_ascii() {

                    let start = start.to_ascii_lowercase();
                    let end = end.to_ascii_lowercase();

                    let start_up = start.to_uppercase();
                    let end_up = end.to_uppercase();

                    // only allow case insensitive matching when
                    // both start and end are within a-z or A-Z
                    if start != start_up && end != end_up {
                        let c = c.to_ascii_lowercase();
                        if c >= start && c <= end {
                            return true;
                        }
                    }
                }

                if c >= start && c <= end {
                    return true;
                }
            }
        }
    }

    false
}

/// A helper function to determine if two chars are (possibly case-insensitively) equal.
fn chars_eq(a: char, b: char, case_sensitive: bool) -> bool {
    if cfg!(windows) && path::windows::is_sep(a) && path::windows::is_sep(b) {
        true
    } else if !case_sensitive && a.is_ascii() && b.is_ascii() {
        // FIXME: work with non-ascii chars properly (issue #9084)
        a.to_ascii_lowercase() == b.to_ascii_lowercase()
    } else {
        a == b
    }
}


/// Configuration options to modify the behaviour of `Pattern::matches_with(..)`
#[allow(missing_copy_implementations)]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MatchOptions {
    /// Whether or not patterns should be matched in a case-sensitive manner. This
    /// currently only considers upper/lower case relationships between ASCII characters,
    /// but in future this might be extended to work with Unicode.
    pub case_sensitive: bool,

    /// If this is true then path-component separator characters (e.g. `/` on Posix)
    /// must be matched by a literal `/`, rather than by `*` or `?` or `[...]`
    pub require_literal_separator: bool,

    /// If this is true then paths that contain components that start with a `.` will
    /// not match unless the `.` appears literally in the pattern: `*`, `?` or `[...]`
    /// will not match. This is useful because such files are conventionally considered
    /// hidden on Unix systems and it might be desirable to skip them when listing files.
    pub require_literal_leading_dot: bool
}

impl MatchOptions {

     /// Constructs a new `MatchOptions` with default field values. This is used
     /// when calling functions that do not take an explicit `MatchOptions` parameter.
     ///
     /// This function always returns this value:
     ///
     /// ```rust,ignore
     /// MatchOptions {
     ///     case_sensitive: true,
     ///     require_literal_separator: false.
     ///     require_literal_leading_dot: false
     /// }
     /// ```
    pub fn new() -> MatchOptions {
        MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false
        }
    }

}

#[cfg(test)]
mod test {
    use std::os;
    use super::{glob, Pattern, MatchOptions};

    #[test]
    fn test_wildcard_errors() {
        assert!(Pattern::new("a/**b").unwrap_err().pos == 4);
        assert!(Pattern::new("a/bc**").unwrap_err().pos == 3);
        assert!(Pattern::new("a/*****").unwrap_err().pos == 4);
        assert!(Pattern::new("a/b**c**d").unwrap_err().pos == 2);
        assert!(Pattern::new("a**b").unwrap_err().pos == 0);
    }

    #[test]
    fn test_unclosed_bracket_errors() {
        assert!(Pattern::new("abc[def").unwrap_err().pos == 3);
        assert!(Pattern::new("abc[!def").unwrap_err().pos == 3 );
        assert!(Pattern::new("abc[").unwrap_err().pos == 3);
        assert!(Pattern::new("abc[!").unwrap_err().pos == 3);
        assert!(Pattern::new("abc[d").unwrap_err().pos == 3);
        assert!(Pattern::new("abc[!d").unwrap_err().pos == 3);
        assert!(Pattern::new("abc[]").unwrap_err().pos == 3);
        assert!(Pattern::new("abc[!]").unwrap_err().pos == 3);
    }

    #[test]
    fn test_glob_errors() {
        assert!(glob("a/**b").err().unwrap().pos == 4);
        assert!(glob("abc[def").err().unwrap().pos == 3);
    }

    #[test]
    fn test_absolute_pattern() {
        // assume that the filesystem is not empty!
        assert!(glob("/*").unwrap().next().is_some());
        assert!(glob("//").unwrap().next().is_some());

        // check windows absolute paths with host/device components
        let root_with_device = os::getcwd().unwrap().root_path().unwrap().join("*");
        // FIXME (#9639): This needs to handle non-utf8 paths
        assert!(glob(root_with_device.as_str().unwrap()).unwrap().next().is_some());
    }

    #[test]
    fn test_wildcards() {
        assert!(Pattern::new("a*b").unwrap().matches("a_b"));
        assert!(Pattern::new("a*b*c").unwrap().matches("abc"));
        assert!(!Pattern::new("a*b*c").unwrap().matches("abcd"));
        assert!(Pattern::new("a*b*c").unwrap().matches("a_b_c"));
        assert!(Pattern::new("a*b*c").unwrap().matches("a___b___c"));
        assert!(Pattern::new("abc*abc*abc").unwrap().matches("abcabcabcabcabcabcabc"));
        assert!(!Pattern::new("abc*abc*abc").unwrap().matches("abcabcabcabcabcabcabca"));
        assert!(Pattern::new("a*a*a*a*a*a*a*a*a").unwrap().matches("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(Pattern::new("a*b[xyz]c*d").unwrap().matches("abxcdbxcddd"));
    }

    #[test]
    fn test_recursive_wildcards() {
        let pat = Pattern::new("some/**/needle.txt").unwrap();
        assert!(pat.matches("some/needle.txt"));
        assert!(pat.matches("some/one/needle.txt"));
        assert!(pat.matches("some/one/two/needle.txt"));
        assert!(pat.matches("some/other/needle.txt"));
        assert!(!pat.matches("some/other/notthis.txt"));

        // a single ** should be valid, for globs
        assert!(Pattern::new("**").unwrap().is_recursive);

        // collapse consecutive wildcards
        let pat = Pattern::new("some/**/**/needle.txt").unwrap();
        assert!(pat.matches("some/needle.txt"));
        assert!(pat.matches("some/one/needle.txt"));
        assert!(pat.matches("some/one/two/needle.txt"));
        assert!(pat.matches("some/other/needle.txt"));
        assert!(!pat.matches("some/other/notthis.txt"));

        // ** can begin the pattern
        let pat = Pattern::new("**/test").unwrap();
        assert!(pat.matches("one/two/test"));
        assert!(pat.matches("one/test"));
        assert!(pat.matches("test"));

        // /** can begin the pattern
        let pat = Pattern::new("/**/test").unwrap();
        assert!(pat.matches("/one/two/test"));
        assert!(pat.matches("/one/test"));
        assert!(pat.matches("/test"));
        assert!(!pat.matches("/one/notthis"));
        assert!(!pat.matches("/notthis"));
    }

    #[test]
    fn test_lots_of_files() {
        // this is a good test because it touches lots of differently named files
        glob("/*/*/*/*").unwrap().skip(10000).next();
    }

    #[test]
    fn test_range_pattern() {

        let pat = Pattern::new("a[0-9]b").unwrap();
        for i in range(0u, 10) {
            assert!(pat.matches(format!("a{}b", i).as_slice()));
        }
        assert!(!pat.matches("a_b"));

        let pat = Pattern::new("a[!0-9]b").unwrap();
        for i in range(0u, 10) {
            assert!(!pat.matches(format!("a{}b", i).as_slice()));
        }
        assert!(pat.matches("a_b"));

        let pats = ["[a-z123]", "[1a-z23]", "[123a-z]"];
        for &p in pats.iter() {
            let pat = Pattern::new(p).unwrap();
            for c in "abcdefghijklmnopqrstuvwxyz".chars() {
                assert!(pat.matches(c.to_string().as_slice()));
            }
            for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars() {
                let options = MatchOptions {case_sensitive: false, .. MatchOptions::new()};
                assert!(pat.matches_with(c.to_string().as_slice(), &options));
            }
            assert!(pat.matches("1"));
            assert!(pat.matches("2"));
            assert!(pat.matches("3"));
        }

        let pats = ["[abc-]", "[-abc]", "[a-c-]"];
        for &p in pats.iter() {
            let pat = Pattern::new(p).unwrap();
            assert!(pat.matches("a"));
            assert!(pat.matches("b"));
            assert!(pat.matches("c"));
            assert!(pat.matches("-"));
            assert!(!pat.matches("d"));
        }

        let pat = Pattern::new("[2-1]").unwrap();
        assert!(!pat.matches("1"));
        assert!(!pat.matches("2"));

        assert!(Pattern::new("[-]").unwrap().matches("-"));
        assert!(!Pattern::new("[!-]").unwrap().matches("-"));
    }

    #[test]
    fn test_pattern_matches() {
        let txt_pat = Pattern::new("*hello.txt").unwrap();
        assert!(txt_pat.matches("hello.txt"));
        assert!(txt_pat.matches("gareth_says_hello.txt"));
        assert!(txt_pat.matches("some/path/to/hello.txt"));
        assert!(txt_pat.matches("some\\path\\to\\hello.txt"));
        assert!(txt_pat.matches("/an/absolute/path/to/hello.txt"));
        assert!(!txt_pat.matches("hello.txt-and-then-some"));
        assert!(!txt_pat.matches("goodbye.txt"));

        let dir_pat = Pattern::new("*some/path/to/hello.txt").unwrap();
        assert!(dir_pat.matches("some/path/to/hello.txt"));
        assert!(dir_pat.matches("a/bigger/some/path/to/hello.txt"));
        assert!(!dir_pat.matches("some/path/to/hello.txt-and-then-some"));
        assert!(!dir_pat.matches("some/other/path/to/hello.txt"));
    }

    #[test]
    fn test_pattern_escape() {
        let s = "_[_]_?_*_!_";
        assert_eq!(Pattern::escape(s), "_[[]_[]]_[?]_[*]_!_".to_string());
        assert!(Pattern::new(Pattern::escape(s).as_slice()).unwrap().matches(s));
    }

    #[test]
    fn test_pattern_matches_case_insensitive() {

        let pat = Pattern::new("aBcDeFg").unwrap();
        let options = MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: false
        };

        assert!(pat.matches_with("aBcDeFg", &options));
        assert!(pat.matches_with("abcdefg", &options));
        assert!(pat.matches_with("ABCDEFG", &options));
        assert!(pat.matches_with("AbCdEfG", &options));
    }

    #[test]
    fn test_pattern_matches_case_insensitive_range() {

        let pat_within = Pattern::new("[a]").unwrap();
        let pat_except = Pattern::new("[!a]").unwrap();

        let options_case_insensitive = MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: false
        };
        let options_case_sensitive = MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false
        };

        assert!(pat_within.matches_with("a", &options_case_insensitive));
        assert!(pat_within.matches_with("A", &options_case_insensitive));
        assert!(!pat_within.matches_with("A", &options_case_sensitive));

        assert!(!pat_except.matches_with("a", &options_case_insensitive));
        assert!(!pat_except.matches_with("A", &options_case_insensitive));
        assert!(pat_except.matches_with("A", &options_case_sensitive));
    }

    #[test]
    fn test_pattern_matches_require_literal_separator() {

        let options_require_literal = MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: false
        };
        let options_not_require_literal = MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false
        };

        assert!(Pattern::new("abc/def").unwrap().matches_with("abc/def", &options_require_literal));
        assert!(!Pattern::new("abc?def").unwrap().matches_with("abc/def", &options_require_literal));
        assert!(!Pattern::new("abc*def").unwrap().matches_with("abc/def", &options_require_literal));
        assert!(!Pattern::new("abc[/]def").unwrap().matches_with("abc/def", &options_require_literal));

        assert!(Pattern::new("abc/def").unwrap().matches_with("abc/def", &options_not_require_literal));
        assert!(Pattern::new("abc?def").unwrap().matches_with("abc/def", &options_not_require_literal));
        assert!(Pattern::new("abc*def").unwrap().matches_with("abc/def", &options_not_require_literal));
        assert!(Pattern::new("abc[/]def").unwrap().matches_with("abc/def", &options_not_require_literal));
    }

    #[test]
    fn test_pattern_matches_require_literal_leading_dot() {

        let options_require_literal_leading_dot = MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: true
        };
        let options_not_require_literal_leading_dot = MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false
        };

        let f = |&: options| Pattern::new("*.txt").unwrap().matches_with(".hello.txt", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(!f(&options_require_literal_leading_dot));

        let f = |&: options| Pattern::new(".*.*").unwrap().matches_with(".hello.txt", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(f(&options_require_literal_leading_dot));

        let f = |&: options| Pattern::new("aaa/bbb/*").unwrap().matches_with("aaa/bbb/.ccc", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(!f(&options_require_literal_leading_dot));

        let f = |&: options| Pattern::new("aaa/bbb/*").unwrap().matches_with("aaa/bbb/c.c.c.", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(f(&options_require_literal_leading_dot));

        let f = |&: options| Pattern::new("aaa/bbb/.*").unwrap().matches_with("aaa/bbb/.ccc", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(f(&options_require_literal_leading_dot));

        let f = |&: options| Pattern::new("aaa/?bbb").unwrap().matches_with("aaa/.bbb", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(!f(&options_require_literal_leading_dot));

        let f = |&: options| Pattern::new("aaa/[.]bbb").unwrap().matches_with("aaa/.bbb", options);
        assert!(f(&options_not_require_literal_leading_dot));
        assert!(!f(&options_require_literal_leading_dot));
    }

    #[test]
    fn test_matches_path() {
        // on windows, (Path::new("a/b").as_str().unwrap() == "a\\b"), so this
        // tests that / and \ are considered equivalent on windows
        assert!(Pattern::new("a/b").unwrap().matches_path(&Path::new("a/b")));
    }
}
