use clap_complete::CompleteEnv;
use clap_complete::engine::{
    ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter,
};
use clap_complete::env::{Bash, Elvish, EnvCompleter, Fish, Powershell, Shells, Zsh};
use gix::bstr::ByteSlice;
use gix::status::index_worktree::iter::Summary;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;

pub fn complete(factory: impl Fn() -> clap::Command) {
    let zsh = ZshWithCompinit;
    let shells: [&dyn EnvCompleter; 5] = [&Bash, &Elvish, &Fish, &Powershell, &zsh];
    CompleteEnv::with_factory(factory)
        .shells(Shells(&shells))
        .complete();
}

struct ZshWithCompinit;

impl EnvCompleter for ZshWithCompinit {
    fn name(&self) -> &'static str {
        "zsh"
    }

    fn is(&self, name: &str) -> bool {
        name == "zsh"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        writeln!(
            buf,
            "if (( ! $+functions[compdef] )); then\n  autoload -Uz compinit\n  compinit\nfi"
        )?;
        let mut registration = Vec::new();
        Zsh.write_registration(var, name, bin, completer, &mut registration)?;
        let registration = String::from_utf8(registration)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        buf.write_all(customize_zsh_registration(registration).as_bytes())
    }

    fn write_complete(
        &self,
        cmd: &mut clap::Command,
        args: Vec<std::ffi::OsString>,
        current_dir: Option<&std::path::Path>,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        let mut completions = Vec::new();
        Zsh.write_complete(cmd, args, current_dir, &mut completions)?;
        buf.write_all(&prepare_zsh_completions(&completions))
    }
}
fn customize_zsh_registration(registration: String) -> String {
    registration
        .replace(
            "        local -a dirs=()\n        local -a other=()",
            "        local -a filter_dirs=()\n        local -a filter_values=()\n        local -a dirs=()\n        local -a other=()",
        )
        .replace(
            "        for completion in $completions; do\n            local value=",
            "        for completion in $completions; do\n            if [[ \"$completion\" == :*/ ]]; then\n                filter_dirs+=(\"${completion%/}\")\n                continue\n            elif [[ \"$completion\" == :* ]]; then\n                filter_values+=(\"$completion\")\n                continue\n            fi\n            local value=",
        )
        .replace(
            "        [[ -n $dirs ]] && _describe -V 'values' dirs -S '/' -r '/'",
            "        [[ -n $filter_dirs ]] && compadd -U -Q -S '/' -r '/' -- \"${filter_dirs[@]}\"\n        [[ -n $filter_values ]] && compadd -U -Q -- \"${filter_values[@]}\"\n        [[ -n $dirs ]] && _describe -V 'values' dirs -S '/' -r '/'",
        )
}

fn prepare_zsh_completions(completions: &[u8]) -> Vec<u8> {
    let mut prepared = Vec::with_capacity(completions.len());
    for line in completions.split_inclusive(|byte| *byte == b'\n') {
        let (line, newline) = line
            .strip_suffix(b"\n")
            .map_or((line, false), |line| (line, true));
        let mut escaped = false;
        let delimiter = line.iter().position(|byte| {
            if *byte == b':' && !escaped {
                return true;
            }
            escaped = *byte == b'\\' && !escaped;
            if *byte != b'\\' {
                escaped = false;
            }
            false
        });
        let value_end = delimiter.unwrap_or(line.len());
        let value = &line[..value_end];
        if value.starts_with(b"\\:") {
            let mut index = 0;
            while index < value_end {
                if line[index] == b'\\' && line.get(index + 1) == Some(&b':') {
                    prepared.push(b':');
                    index += 2;
                } else {
                    if line[index] == b'!' {
                        prepared.push(b'\\');
                    }
                    prepared.push(line[index]);
                    index += 1;
                }
            }
        } else {
            prepared.extend_from_slice(line);
        }
        if newline {
            prepared.push(b'\n');
        }
    }
    prepared
}

#[derive(Clone, Copy)]
enum SnapshotSource {
    #[cfg(test)]
    None,
    Head,
    Worktree,
}

#[derive(Clone, Copy)]
enum RefMode {
    Revision { snapshots: bool },
    Branch,
    Full,
    Writable,
    Refspec,
}

pub fn filter_syntax() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| complete_filter(current, SnapshotSource::Head))
}

pub fn filter_head() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| complete_filter(current, SnapshotSource::Head))
}

pub fn filter_worktree() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| complete_filter(current, SnapshotSource::Worktree))
}

pub fn filter_or_file() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| {
        if completion_words().iter().any(|word| word == "--file") {
            PathCompleter::file().complete(current)
        } else {
            complete_filter(current, SnapshotSource::Head)
        }
    })
}

pub fn revision() -> ArgValueCompleter {
    ref_completer(RefMode::Revision { snapshots: true })
}
pub fn revision_binding() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| {
        let Some(current) = current.to_str() else {
            return Vec::new();
        };
        let Some((name, value)) = current.split_once('=') else {
            return Vec::new();
        };
        complete_ref(OsStr::new(value), RefMode::Revision { snapshots: true })
            .into_iter()
            .map(|completion| {
                candidate(
                    format!("{name}={}", completion.get_value().to_string_lossy()),
                    "revision binding",
                )
            })
            .collect()
    })
}

pub fn git_ref() -> ArgValueCompleter {
    ref_completer(RefMode::Revision { snapshots: false })
}

pub fn branch() -> ArgValueCompleter {
    ref_completer(RefMode::Branch)
}

pub fn full_ref() -> ArgValueCompleter {
    ref_completer(RefMode::Full)
}

pub fn writable_ref() -> ArgValueCompleter {
    ref_completer(RefMode::Writable)
}

pub fn refspec() -> ArgValueCompleter {
    ref_completer(RefMode::Refspec)
}

pub fn remote() -> ArgValueCompleter {
    ArgValueCompleter::new(complete_remote)
}

fn ref_completer(mode: RefMode) -> ArgValueCompleter {
    ArgValueCompleter::new(move |current: &OsStr| complete_ref(current, mode))
}

fn candidate(value: impl Into<String>, help: &'static str) -> CompletionCandidate {
    CompletionCandidate::new(value.into()).help(Some(help.into()))
}

fn complete_filter(current: &OsStr, source: SnapshotSource) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    let current = if current.contains("\\!") {
        std::borrow::Cow::Owned(current.replace("\\!", "!"))
    } else {
        std::borrow::Cow::Borrowed(current)
    };
    let current = current.as_ref();
    if !current.is_empty() && !current.contains(':') {
        return Vec::new();
    }

    let start = active_filter_start(current).unwrap_or(current.len());
    let prefix = &current[..start];
    let primitive = &current[start..];
    let mut candidates = Vec::new();

    const STABLE: &[(&str, &str)] = &[
        (":/", "select a subdirectory"),
        ("::", "select a file or directory"),
        (":+", "apply a stored .josh filter"),
        (":workspace=", "apply a workspace.josh filter"),
        (":prefix=", "place the input under a prefix"),
        (":[:", "compose filters"),
        (":exclude[:", "exclude paths selected by a filter"),
        (":invert[:", "invert a filter"),
        (":linear[:", "linearize filtered history"),
        (":empty", "produce an empty tree"),
        (":nop", "leave the input unchanged"),
        (":prune=trivial-merge", "remove trivial merge commits"),
        (":unsign", "remove commit signatures"),
        (":rev(", "select filters by revision"),
        (":replace(", "replace text with regular expressions"),
        (":!", "run a stored Starlark filter"),
    ];
    for &(value, help) in STABLE {
        push_filter_candidate(&mut candidates, prefix, primitive, value, help);
    }

    if josh_core::filter::experimental_features_enabled() {
        const EXPERIMENTAL: &[(&str, &str)] = &[
            (":&", "replace a path with an object reference"),
            (":#", "dereference an object or capture a tree id"),
        ];
        for &(value, help) in EXPERIMENTAL {
            push_filter_candidate(&mut candidates, prefix, primitive, value, help);
        }
    }

    let inventory = filter_revision(source)
        .map(PathInventory::load)
        .unwrap_or_default();
    if let Some(typed) = primitive.strip_prefix(":/") {
        add_path_candidates(
            &mut candidates,
            prefix,
            ":/",
            typed,
            inventory.dirs.iter().map(|path| format!("{path}/")),
            "directory",
        );
    } else if let Some(typed) = primitive.strip_prefix("::") {
        let (base, typed) = typed
            .rsplit_once('=')
            .map_or(("::".to_owned(), typed), |(destination, source)| {
                (format!("::{destination}="), source)
            });
        add_path_candidates(
            &mut candidates,
            prefix,
            &base,
            typed,
            inventory
                .dirs
                .iter()
                .map(|path| format!("{path}/"))
                .chain(inventory.files.iter().cloned()),
            "path",
        );
    } else if let Some(typed) = primitive.strip_prefix(":+") {
        let mut stored = inventory
            .files
            .iter()
            .filter_map(|path| path.strip_suffix(".josh").map(str::to_owned))
            .collect::<BTreeSet<_>>();
        add_parent_dirs(&mut stored);
        add_path_candidates(
            &mut candidates,
            prefix,
            ":+",
            typed,
            stored.iter().map(String::as_str),
            "stored filter",
        );
    } else if let Some(typed) = primitive.strip_prefix(":workspace=") {
        let mut workspaces = inventory
            .files
            .iter()
            .filter_map(|path| path.strip_suffix("/workspace.josh").map(str::to_owned))
            .collect::<BTreeSet<_>>();
        add_parent_dirs(&mut workspaces);
        add_path_candidates(
            &mut candidates,
            prefix,
            ":workspace=",
            typed,
            workspaces.iter().map(String::as_str),
            "workspace",
        );
    } else if let Some(typed) = primitive.strip_prefix(":!") {
        let mut scripts = inventory
            .files
            .iter()
            .filter_map(|path| path.strip_suffix(".star").map(str::to_owned))
            .collect::<BTreeSet<_>>();
        add_parent_dirs(&mut scripts);
        add_path_candidates(
            &mut candidates,
            prefix,
            ":!",
            typed,
            scripts.iter().map(String::as_str),
            "Starlark filter",
        );
    }

    candidates.sort();
    candidates.dedup_by(|left, right| left.get_value() == right.get_value());
    candidates
}

fn push_filter_candidate(
    candidates: &mut Vec<CompletionCandidate>,
    prefix: &str,
    primitive: &str,
    value: &str,
    help: &'static str,
) {
    if value.starts_with(primitive) {
        candidates.push(candidate(format!("{prefix}{value}"), help));
    }
}

fn active_filter_start(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut active = None;
    let mut previous_colon = false;

    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            previous_colon = false;
            continue;
        }
        if ch == '\\' && quote.is_some() {
            escaped = true;
            previous_colon = false;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            }
            previous_colon = false;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            previous_colon = false;
        } else if ch == ':' {
            if !previous_colon {
                active = Some(index);
            }
            previous_colon = true;
        } else {
            previous_colon = false;
        }
    }
    active
}

fn filter_revision(source: SnapshotSource) -> Option<String> {
    match source {
        #[cfg(test)]
        SnapshotSource::None => None,
        SnapshotSource::Head => Some("HEAD".to_owned()),
        SnapshotSource::Worktree => Some(selected_revision().unwrap_or_else(|| ".".to_owned())),
    }
}

fn selected_revision() -> Option<String> {
    let words = completion_words();
    for (index, word) in words.iter().enumerate() {
        if let Some(value) = word.strip_prefix("--revision=") {
            return Some(value.to_owned());
        }
        if (word == "--revision" || word == "-r")
            && let Some(value) = words.get(index + 1)
        {
            return Some(value.clone());
        }
    }
    None
}

fn completion_words() -> Vec<String> {
    let args = std::env::args_os().collect::<Vec<_>>();
    let start = args
        .iter()
        .position(|arg| arg == "--")
        .map_or(1, |index| index + 1);
    args.into_iter()
        .skip(start)
        .filter_map(|arg| arg.into_string().ok())
        .collect()
}

#[derive(Default)]
struct PathInventory {
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
}

impl PathInventory {
    fn load(revision: String) -> Self {
        let Some(repo) = discover_repository() else {
            return Self::default();
        };
        let files = match revision.as_str() {
            "." => worktree_paths(&repo),
            "+" => index_paths(&repo),
            revision => tree_paths(&repo, revision),
        };
        let mut dirs = BTreeSet::new();
        for file in &files {
            let mut path = Path::new(file);
            while let Some(parent) = path.parent() {
                let Some(parent) = parent.to_str() else {
                    break;
                };
                if parent.is_empty() {
                    break;
                }
                dirs.insert(parent.to_owned());
                path = parent.as_ref();
            }
        }
        Self { files, dirs }
    }
}

fn discover_repository() -> Option<gix::Repository> {
    gix::discover_with_environment_overrides(std::env::current_dir().ok()?).ok()
}

fn tree_paths(repo: &gix::Repository, revision: &str) -> BTreeSet<String> {
    let Ok(id) = repo.rev_parse_single(revision) else {
        return BTreeSet::new();
    };
    let Ok(object) = id.object() else {
        return BTreeSet::new();
    };
    let Ok(tree) = object.peel_to_tree() else {
        return BTreeSet::new();
    };

    let mut files = BTreeSet::new();
    let mut pending = vec![(String::new(), tree)];
    while let Some((prefix, tree)) = pending.pop() {
        for entry in tree.iter() {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(filename) = entry.filename().to_str() else {
                continue;
            };
            let path = if prefix.is_empty() {
                filename.to_owned()
            } else {
                format!("{prefix}/{filename}")
            };
            if entry.mode().is_tree() {
                let Ok(object) = entry.object() else {
                    continue;
                };
                let Ok(tree) = object.try_into_tree() else {
                    continue;
                };
                pending.push((path, tree));
            } else {
                files.insert(path);
            }
        }
    }
    files
}

fn index_paths(repo: &gix::Repository) -> BTreeSet<String> {
    let Ok(index) = repo.index_or_empty() else {
        return BTreeSet::new();
    };
    index
        .entries()
        .iter()
        .filter_map(|entry| entry.path(&index).to_str().ok().map(str::to_owned))
        .collect()
}

fn worktree_paths(repo: &gix::Repository) -> BTreeSet<String> {
    let mut files = tree_paths(repo, "HEAD");
    let Ok(head_tree) = repo.head_tree_id() else {
        return index_paths(repo);
    };
    let Ok(head_index) = repo.index_from_tree(&head_tree) else {
        return files;
    };
    let Ok(status) = repo.status(gix::progress::Discard) else {
        return files;
    };
    let Ok(status) = status
        .index(gix::worktree::IndexPersistedOrInMemory::InMemory(
            head_index,
        ))
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_submodules(None)
        .into_index_worktree_iter(Vec::new())
    else {
        return files;
    };
    for item in status {
        let Ok(item) = item else {
            continue;
        };
        let Ok(path) = item.rela_path().to_str() else {
            continue;
        };
        match item.summary() {
            Some(Summary::Removed) => {
                files.remove(path);
            }
            Some(_) => {
                files.insert(path.to_owned());
            }
            None => {}
        }
    }
    files
}

fn add_parent_dirs(paths: &mut BTreeSet<String>) {
    let existing = paths.iter().cloned().collect::<Vec<_>>();
    for path in existing {
        let mut path = Path::new(&path);
        while let Some(parent) = path.parent() {
            let Some(parent) = parent.to_str() else {
                break;
            };
            if parent.is_empty() {
                break;
            }
            paths.insert(format!("{parent}/"));
            path = parent.as_ref();
        }
    }
}

fn add_path_candidates(
    candidates: &mut Vec<CompletionCandidate>,
    filter_prefix: &str,
    primitive_prefix: &str,
    typed: &str,
    paths: impl IntoIterator<Item = impl AsRef<str>>,
    help: &'static str,
) {
    let parent = typed.rsplit_once('/').map_or("", |(parent, _)| parent);
    for path in paths {
        let path = path.as_ref();
        let normalized = path.trim_end_matches('/');
        let candidate_parent = normalized.rsplit_once('/').map_or("", |(parent, _)| parent);
        if path.starts_with(typed) && candidate_parent == parent {
            candidates.push(candidate(
                format!("{filter_prefix}{primitive_prefix}{path}"),
                help,
            ));
        }
    }
}

fn complete_ref(current: &OsStr, mode: RefMode) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    if matches!(mode, RefMode::Refspec) {
        return complete_refspec(current);
    }

    let refs = refs();
    let (base, suffix) = split_revision_suffix(current);
    let full = base.starts_with("refs/") || matches!(mode, RefMode::Full | RefMode::Writable);
    let mut values = BTreeSet::new();

    if matches!(mode, RefMode::Revision { .. }) {
        values.insert(("HEAD".to_owned(), "symbolic ref"));
    }
    if matches!(mode, RefMode::Revision { snapshots: true }) {
        values.insert((".".to_owned(), "working tree"));
        values.insert(("+".to_owned(), "index"));
    }

    for reference in refs {
        if full {
            values.insert((reference, "ref"));
        } else if let Some(branch) = reference.strip_prefix("refs/heads/") {
            values.insert((branch.to_owned(), "branch"));
        } else if !matches!(mode, RefMode::Branch)
            && let Some(tag) = reference.strip_prefix("refs/tags/")
        {
            values.insert((tag.to_owned(), "tag"));
        } else if !matches!(mode, RefMode::Branch)
            && let Some(remote) = reference.strip_prefix("refs/remotes/")
            && !remote.ends_with("/HEAD")
        {
            values.insert((remote.to_owned(), "remote branch"));
        }
    }

    if matches!(mode, RefMode::Writable) {
        values.insert(("refs/heads/".to_owned(), "branch namespace"));
        values.insert(("refs/tags/".to_owned(), "tag namespace"));
        values.insert(("refs/josh/".to_owned(), "Josh ref namespace"));
    }

    values
        .into_iter()
        .filter(|(value, _)| value.starts_with(base))
        .map(|(value, help)| candidate(format!("{value}{suffix}"), help))
        .collect()
}

fn complete_refspec(current: &str) -> Vec<CompletionCandidate> {
    let (force, current) = current
        .strip_prefix('+')
        .map_or(("", current), |current| ("+", current));
    if let Some((source, destination)) = current.split_once(':') {
        let full = destination.starts_with("refs/");
        let mut destinations = ref_names(full);
        if full {
            destinations.extend(["refs/heads/", "refs/tags/", "refs/josh/"].map(str::to_owned));
        }
        destinations
            .into_iter()
            .filter(|reference| reference.starts_with(destination))
            .map(|reference| candidate(format!("{force}{source}:{reference}"), "destination ref"))
            .collect()
    } else {
        ref_names(current.starts_with("refs/"))
            .into_iter()
            .filter(|reference| reference.starts_with(current))
            .map(|reference| candidate(format!("{force}{reference}"), "source ref"))
            .collect()
    }
}

fn ref_names(full: bool) -> BTreeSet<String> {
    let mut names = BTreeSet::from(["HEAD".to_owned()]);
    for reference in refs() {
        if full {
            names.insert(reference);
        } else if let Some(branch) = reference.strip_prefix("refs/heads/") {
            names.insert(branch.to_owned());
        } else if let Some(tag) = reference.strip_prefix("refs/tags/") {
            names.insert(tag.to_owned());
        } else if let Some(remote) = reference.strip_prefix("refs/remotes/")
            && !remote.ends_with("/HEAD")
        {
            names.insert(remote.to_owned());
        }
    }
    names
}

fn split_revision_suffix(value: &str) -> (&str, &str) {
    value
        .find(['~', '^'])
        .map_or((value, ""), |index| value.split_at(index))
}

fn complete_remote(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    let Some(repo) = discover_repository() else {
        return Vec::new();
    };
    repo.remote_names()
        .into_iter()
        .filter_map(|remote| remote.to_str().ok().map(str::to_owned))
        .filter(|remote| remote.starts_with(current))
        .map(|remote| candidate(remote, "Git remote"))
        .collect()
}

fn refs() -> BTreeSet<String> {
    let Some(repo) = discover_repository() else {
        return BTreeSet::new();
    };
    let Ok(platform) = repo.references() else {
        return BTreeSet::new();
    };
    let Ok(iter) = platform.all() else {
        return BTreeSet::new();
    };
    let mut refs = BTreeSet::new();
    for reference in iter {
        let Ok(reference) = reference else {
            continue;
        };
        let Ok(name) = reference.name().as_bstr().to_str() else {
            continue;
        };
        refs.insert(name.to_owned());
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(candidates: Vec<CompletionCandidate>) -> Vec<String> {
        candidates
            .into_iter()
            .map(|candidate| candidate.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn zsh_registration_initializes_completion_system() {
        let mut registration = Vec::new();
        ZshWithCompinit
            .write_registration("COMPLETE", "josh", "josh", "josh", &mut registration)
            .unwrap();
        let registration = String::from_utf8(registration).unwrap();
        assert!(registration.starts_with(
            "if (( ! $+functions[compdef] )); then\n  autoload -Uz compinit\n  compinit\nfi\n"
        ));
        assert!(registration.contains("compdef _clap_dynamic_completer_josh josh"));
        assert!(registration.contains("local -a filter_dirs=()"));
        assert!(registration.contains("compadd -U -Q -S '/' -r '/' -- \"${filter_dirs[@]}\""));
        assert_eq!(
            prepare_zsh_completions(
                b"\\:+ws/:stored filter\n\\:+ws/test:stored filter\n\\:!ws/build:Starlark filter",
            ),
            b":+ws/\n:+ws/test\n:\\!ws/build",
        );
    }

    #[test]
    fn completes_filter_starters_and_chains() {
        let starters = values(complete_filter(OsStr::new(":wor"), SnapshotSource::None));
        assert_eq!(starters, [":workspace="]);

        let starlark = values(complete_filter(OsStr::new(":!"), SnapshotSource::None));
        assert_eq!(starlark, [":!"]);

        let escaped_starlark = values(complete_filter(OsStr::new(":\\!"), SnapshotSource::None));
        assert_eq!(escaped_starlark, [":!"]);

        let chained = values(complete_filter(
            OsStr::new(":/src:pre"),
            SnapshotSource::None,
        ));
        assert_eq!(chained, [":/src:prefix="]);
    }

    #[test]
    fn finds_active_filter_outside_quoted_content() {
        let filter = r#":replace("a:b":"c"):/do"#;
        let start = active_filter_start(filter).unwrap();
        assert_eq!(&filter[start..], ":/do");
        assert_eq!(active_filter_start("::path"), Some(0));
    }

    #[test]
    fn path_completion_only_returns_immediate_children() {
        let mut candidates = Vec::new();
        add_path_candidates(
            &mut candidates,
            "",
            "::",
            "src/l",
            ["src/lib/", "src/lib/deep/", "src/main.rs"],
            "path",
        );
        assert_eq!(values(candidates), ["::src/lib/"]);
    }

    #[test]
    fn preserves_revision_suffixes() {
        assert_eq!(split_revision_suffix("main~2"), ("main", "~2"));
        assert_eq!(split_revision_suffix("topic^"), ("topic", "^"));
        assert_eq!(split_revision_suffix("HEAD"), ("HEAD", ""));
    }

    #[test]
    fn completes_refspec_head_and_destination_namespaces() {
        assert!(values(complete_refspec("HE")).contains(&"HEAD".to_owned()));
        let destinations = values(complete_refspec("HEAD:refs/h"));
        assert!(destinations.contains(&"HEAD:refs/heads/".to_owned()));
        assert!(
            destinations
                .iter()
                .all(|candidate| candidate.starts_with("HEAD:refs/heads/"))
        );
    }
}
