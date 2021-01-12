[![Build Status](https://github.com/esrlabs/josh/workflows/Rust/badge.svg?branch=master)](https://github.com/esrlabs/josh/actions)
[![Documentation Status](https://readthedocs.org/projects/josh/badge/?version=latest)](https://josh.readthedocs.io/en/latest/?badge=latest)

Just One Single History
=======================

The Josh project is aimed at supporting trunk based development in a Git monorepo.

Proxy
-----

The main component of Josh is the `josh-proxy` which enables on the fly virtualization
of Git repositories hosted on an upstream Git host.

On the most basic level this can be used to support partial cloning for Git repositories.

Lets say you want to clone just the *Documentation* subdirectory of Git itself:

```
josh-proxy --local=/tmp/josh --remote=https://github.com& --port=8000
git clone http://localhost:8000/git/git.git:/Documentation.git
```

This will give you a repository containing just the *Documentation* part of the upstream
git tree including its history.

Josh supports read and write access to the repository, so when making changes
to any files in the virtualized repository, you can just commit and push them
like you are used to.

Prefix
------

Another useful transformation that `josh-proxy` can do is moving a whole history into
a subdirectory.
This is essentially what has been described as the *git subtree* approach to integrating
code from multiple repositories.
This makes it very easy to compose a new project out of several existing repositories:

```
git init
git commit -m "initial" --allow-empty
git fetch http://localhost:8000/bla/bla.git:prefix=dependencies/bla.git master:bla
git merge bla --allow-unrelated

git fetch http://localhost:8000/bla/foo.git:prefix=dependencies/foo.git master:foo
git merge foo --allow-unrelated
```

One obvious use case for this feature is to help when switching a project or organization
from a manyrepo setup to a monorepo.

Workspaces
----------

Each virtual repository that Josh exposes is the result of applying a transformation to the
upstream repository.
These transformations can be combined to define a workspace that includes arbitrary parts
of the upstream tree and rearranges them in a different way.

A workspace is defined by creating a directory in the upstream repository with a file
`workspace.josh` inside it.

For example:

```
libs/a = :/modules/a
bin/b = :/tools/b
```

Checking this file in as `/ws_x/workspace.josh` in the upstream will than make the virtual
repository available as:
`http://localhost:8000/upstream.git:workspace=ws_x.git`

And the contents will be all the contents inside the `ws_x` directory in the upstream, plus
the contents defined in the `workspace.josh` file in the specified locations.
