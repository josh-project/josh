# Experimental features

Experimental features are opt-in and must be enabled at runtime by setting the
environment variable `JOSH_EXPERIMENTAL_FEATURES=1`. Their behaviour or syntax
may change in future releases.

## Filters

### Blob insertion **`:$path="content"`**

Inserts a new file at `path` in the output tree with the given literal text as its content.
No newline is appended automatically. The path argument follows the same quoting rules as
other filter arguments: quote with double quotes if the path contains spaces or special
characters.

The inverse of `:$path="content"` is `:exclude[::path]`, which removes the inserted file.

**Examples:**

```
# Insert a file named "VERSION" containing "1.0" at the root
:$VERSION="1.0"

# Insert a file whose name contains a space
:$"release notes.txt"="Initial release"

# Combine with a subdirectory filter to insert the file alongside existing content
:[:/sub1,:$added.txt="hello world"]
```

### Object reference **`:&path`**
Reads the git object at `path` (a file or directory) and replaces its content with a text blob
containing the object's SHA-1 hash. This turns a real file or tree into a lightweight pointer.

If `path` does not exist in the input tree, the filter is a no-op.

Example: `:&sub1` on a commit where `sub1` is a directory produces a file `sub1` whose content
is the 40-character SHA of that directory tree.

### Object dereference **`:#path`**
Reads the SHA-1 hash stored as text in the file at `path` and replaces that file with the git
object the hash points to (a file or a directory tree). This follows the pointer written by
`:&path`.

If `path` does not exist the filter is a no-op. If the content is not a valid SHA or the object
is not present in the repository, an error is returned.

Example: given a file `sub1` whose content is the SHA of a directory tree, `:#sub1` replaces
that file with the actual directory tree at `sub1`.

### Object dereference into subdirectory **`:#/path`**
Dereferences the pointer stored at `path` and then extracts the resulting object directly at the
repository root, discarding the `path` prefix. This is the typical way to restore content that
was previously stored with `:&path`.

Expands to `:#path:/path`. The canonical printed form is the expanded syntax.

Example: `:#/sub1` on a tree where `sub1` holds a SHA of a directory returns that directory's
contents at the root, as if `sub1` never existed.

### Tree ID capture **`:#path[filter]`**
Applies `filter` to the current tree and writes the SHA-1 hash of the resulting tree as a text
file at `path`. The filter itself does not appear in the output — only the hash it produces.

This lets you record a stable, content-addressed reference to a subtree alongside other files.

Example: `:#version.txt[:/sub1]` writes the SHA of the `sub1` directory tree into `version.txt`.

### Starlark filter **`:!path/to/script[context filter]`**
Evaluates a [Starlark](https://github.com/bazelbuild/starlark) script stored in the repository
and uses the filter it produces. The script file is loaded from `path` with a `.star` extension
appended automatically.

The optional `[context filter]` scopes the tree that is visible to the script: the context
filter is applied to the input tree first, and the result is what the script sees as `tree`. The
context filter does not affect the filter that the script returns — it only controls what the
script can read.

The script file itself is always included in the output tree alongside whatever the script's
filter selects.

**Script contract**

The script must assign a `Filter` value to the variable named `filter`. At the start of
execution `filter` is pre-set to a no-op filter, so a minimal script that selects nothing can
simply leave it unchanged, or assign a new value:

```python
filter = filter.subdir("src")
```

**Global variables available in the script**

| Variable | Type     | Description |
|----------|----------|-------------|
| `filter` | `Filter` | Starts as a no-op filter. Assign your result here. |
| `tree`   | `Tree`   | The commit tree (or the context-filtered tree if a context filter was given). |

**Global functions**

| Function | Description |
|----------|-------------|
| `compose([f1, f2, ...])` | Overlay multiple filters, same semantics as `:[f1,f2,...]`. |

**`Filter` methods**

All methods return a new `Filter` and can be chained.

| Method | Description |
|--------|-------------|
| `filter.subdir(path)` | Select a subdirectory and make it the root. |
| `filter.prefix(path)` | Place the tree under a subdirectory prefix. |
| `filter.file(path)` | Select a single file, keeping its path. |
| `filter.rename(dst, src)` | Select `src` and place it at `dst`. |
| `filter.pattern(pattern)` | Select files/directories matching a glob pattern (`*` allowed). |
| `filter.chain(other)` | Apply `other` after this filter. |
| `filter.nop()` | No-op; passes the tree through unchanged. |
| `filter.empty()` | Produce an empty tree. |
| `filter.linear()` | Linearise history (drop merge parents). |
| `filter.workspace(path)` | Apply the workspace filter rooted at `path`. |
| `filter.stored(path)` | Apply the stored filter at `path.josh`. |
| `filter.starlark(path, context_filter)` | Apply another Starlark filter with an optional context filter. |
| `filter.author(name, email)` | Override the commit author. |
| `filter.committer(name, email)` | Override the committer. |
| `filter.message(template)` | Rewrite commit messages using a template. |
| `filter.unsign()` | Strip GPG signatures from commits. |
| `filter.prune_trivial_merge()` | Remove merge commits whose tree equals their first parent. |
| `filter.hook(hook)` | Apply a hook filter. |
| `filter.with_meta(key, value)` | Attach metadata to the filter. |
| `filter.is_nop()` | Returns `True` if the filter is a no-op. |
| `filter.peel()` | Strip metadata from the filter. |

**`Tree` methods**

The `tree` object provides read-only access to the commit tree visible to the script.

| Method | Description |
|--------|-------------|
| `tree.file(path)` | Returns the text content of the file at `path`, or an empty string if absent or binary. |
| `tree.files(path)` | Returns a list of file paths that are direct children of `path`. |
| `tree.dirs(path)` | Returns a list of directory paths that are direct children of `path`. |
| `tree.tree(path)` | Returns a `Tree` object rooted at `path`. |

**Example**

A script that dynamically includes every top-level subdirectory as a prefixed subtree:

```python
# st/config.star
parts = [filter.subdir(d).prefix(d) for d in tree.dirs("")]
filter = compose(parts)
```

Applied with `:!st/config`.
