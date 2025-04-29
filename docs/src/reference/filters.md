
# History filtering

Josh transforms commits by applying filters to them. As any
commit in git represents not just a single state of the file system but also its entire
history, applying a filter to a commit produces an entirely new history.
The result of a filter is a normal git commit and therefore can be filtered again,
making filters chainable.

## Syntax

Filters always begin with a colon and can be chained:

    :filter1:filter2

When used as part of an URL filters cannot contain white space or newlines. When read from a file
however white space can be inserted between filters (not after the leading colon).
Additionally newlines can be used instead of ``,`` inside of composition filters.

Some filters take arguments, and arguments can optionally be quoted using double quotes,
if special characters used by the filter language need to be used (like `:` or space):

    :filter=argument1,"argument2"


## Available filters

### Subdirectory **`:/a`**
Take only the selected subdirectory from the input and make it the root
of the filtered tree.
Note that ``:/a/b`` and ``:/a:/b`` are equivalent ways to get the same result.

### Directory **`::a/`**
A shorthand for the commonly occurring filter combination ``:/a:prefix=a``.

### File **`::a`**
Produces a tree with only the specified file in it's root.
Note that `::a/b` is equivalent to `::a/::b`.

### Prefix **`:prefix=a`**
Take the input tree and place it into subdirectory ``a``.
Note that ``:prefix=a/b`` and ``:prefix=b:prefix=a`` are equivalent.

### Composition **`:[:filter1,:filter2,...,:filterN]`**
Compose a tree by overlaying the outputs of ``:filter1`` ... ``:filterN`` on top of each other.
It is guaranteed that each file will only appear at most once in the output. The first filter
that consumes a file is the one deciding it's mapped location. Therefore the order in which
filters are composed matters.

Inside of a composition ``x=:filter`` can be used as an alternative spelling for
``:filter:prefix=x``.

### Exclusion **`:exclude[:filter]`**
Remove all paths present in the *output* of ``:filter`` from the input tree.
It should generally be avoided to use any filters that change paths and instead only
use filters that select paths without altering them.

### Workspace **`:workspace=a`**
Similar to ``:/a`` but also looks for a ``workspace.josh`` file inside the
specified directory (called the "workspace root").
The resulting tree will contain the contents of the
workspace root as well as additional files specified in the ``workspace.josh`` file.
(see [Workspaces](./workspace.md))

### Text replacement **`:replace("regex_0":"replacement_0",...,"regex_N":"replacement_N")`**
Applies the supplied regular expressions to every file in the input tree.

### Signature removal **`:unsign`**
The default behaviour of Josh is to copy, if it exists, the signature of the original commit in
the filtered commit. This makes the signature invalid, but allows a perfect round-trip: josh will be
able to recreate the original commit from the filtered one.

This behaviour might not be desirable, and this filter drops the signatures from the history.

## Pattern filters

The following filters accept a glob like pattern ``X`` that can contain ``*`` to
match any number of characters. Note that two or more consecutive wildcards (``**``) are not
allowed.

### Match directories **`::X/`**
All matching subdirectories in the input root

### Match files or directories **`::X`**
All matching files or directories in the input root

### Match nested directories **`::**/X/`**
All subdirectories matching the pattern in arbitrarily deep subdirectories of the input

### Match nested files **`::**/X`**
All files matching the pattern in arbitrarily deep subdirectories of the input

## History filters

These filter do not modify git trees, but instead only operate on the commit graph.

### Linearise history **:linear**
Produce a filtered history that does not contain any merge commits. This is done by
simply dropping all parents except the first on every commit.

### Filter specific parts of the history **:rev(<sha_0>:filter_0,...,<sha_N>:filter_N)**
Produce a history where the commits specified by `<sha_N>` are replaced by the result of applying
`:filter_N` to it.

It will appear like `<sha_N>` and all its ancestors are also filtered with `<filter_N>`. If an
ancestor also has a matching entry in the `:rev(...)` it's filter will *replace* `<filter_N>`
for all further ancestors (and so on).

This special value `0000000000000000000000000000000000000000` can be used as a `<sha_n>` to filter
commits that don't match any of the other shas.

### Join multiple histories into one **:join(<sha_0>:filter_0,...,<sha_N>:filter_N)**

Produce the history that would be the result of pushing the passed branches with the
passed filters into the upstream.

Filter order matters
--------------------

Filters are applied in the left-to-right order they are given in the filter specification,
and they are `not` commutative.

For example, this command will filter out just the josh documentation, and store it in a
ref named ``FILTERED_HEAD``:

    $ josh-filter :/docs:prefix=josh-docs

However, `this` command will produce an empty branch:

    $ josh-filter :prefix=josh-docs:/docs

What's happening in the latter command is that because the prefix filter is applied first, the
entire ``josh`` history already lives within the ``josh-docs`` directory, as it was just
transformed to exist there. Thus, to still get the docs, the command would need to be:

    $ josh-filter :prefix=josh-docs:/josh-docs/docs

which will contain the josh documentation at the base of the tree. We've lost the prefix, what
gives?? Because the original git tree was already transformed, and then the subdirectory filter
was applied to pull documentation from ``josh-docs/docs``, the prefix is gone - it was filtered out
again by the subdirectory filter. Thus, the order in which filters are provided is crucial, as each
filter further transforms the latest transformation of the tree.
