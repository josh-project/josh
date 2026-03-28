# Working with workspaces

A workspace is a list of files and folders remapped from a central repository into a new
repository layout. For example, a shared library can be mapped into multiple workspaces,
each placing it at the path that makes sense for that project.

In this guide we'll set up a small monorepo with two libraries and two applications, then
use workspaces to develop against them in isolation.

## The monorepo

Suppose you have a monorepo with this structure:

```
monorepo/
├── application1/
│   └── app.c
├── application2/
│   └── guide.c
├── library1/
│   └── lib1.h
└── library2/
    └── lib2.h
```

Both applications depend on one or both of the shared libraries, but developers working on
each application only want to see the code that's relevant to them.

## Creating a workspace

Clone the monorepo scoped to `application1` using the `:workspace=` filter:

```shell
josh clone https://github.com/myorg/monorepo.git :workspace=application1 ./application1
cd application1
```

The cloned repository contains only the files and history relevant to `application1`:

```
app.c
```

> **Note:** Josh lets you create a workspace out of any directory, even one that doesn't
> exist yet in the monorepo.

## Mapping dependencies with `workspace.josh`

The `workspace.josh` file describes which folders from the central repository should be
mapped into this workspace, and where they should appear.

Since `application1` depends on `library1`, create `workspace.josh` with:

```
modules/lib1 = :/library1
```

Commit the file:

```shell
git add workspace.josh
git commit -m "Map library1 into the application1 workspace"
```

Then push and pull to sync the workspace:

```shell
josh push
josh pull
```

The mapped library has now appeared in the workspace:

```
app.c
modules/
└── lib1/
    └── lib1.h
workspace.josh
```

The history reflects the merge of `library1`'s history into the workspace:

```
*   Map library1 into the application1 workspace
|\
| * Add library1
* Add application1
```

Josh needs to merge the mapped module's history into the workspace so that all commits
are present in both histories. In the central repository, the same commit appears as a
plain linear commit — no merge — in `application1/`:

```
* Map library1 into the application1 workspace
* Add documentation
* Add application2
* Add library2
* Add application1
* Add library1
```

## A second workspace

Let's create a workspace for `application2`, which depends on both libraries:

```shell
cd ..
josh clone https://github.com/myorg/monorepo.git :workspace=application2 ./application2
cd application2
```

Add both dependencies to `workspace.josh`:

```
libs/lib1 = :/library1
libs/lib2 = :/library2
```

Commit, push, and pull:

```shell
git add workspace.josh
git commit -m "Create workspace for application2"
josh push
josh pull
```

The workspace now contains both libraries:

```
guide.c
libs/
├── lib1/
│   └── lib1.h
└── lib2/
    └── lib2.h
workspace.josh
```

Because we added both dependencies in a single commit, the history contains just one
merge commit, pulling in the history of both libraries:

```
*   Create workspace for application2
|\
| * Add library2
| * Add library1
* Add application2
```

## Pushing a change back to the monorepo

While working in `application2`, you notice a bug in `library1`. Fix it and commit as
usual:

```shell
# edit libs/lib1/lib1.h
git commit -a -m "Fix bug in lib1"
josh push
```

Josh reverses the workspace filter and writes the change directly to `library1/` in the
central repository. No special tooling is needed — it's a regular commit from the
perspective of everyone else working on the monorepo.

## Pulling a change from another workspace

A developer working in `application1` can now pull the fix:

```shell
cd ../application1
josh pull
```

The fix appears in `modules/lib1/` exactly as if it had been committed there directly:

```
* Fix bug in lib1
*   Map library1 into the application1 workspace
|\
| * Add library1
* Add application1
```

Changes flow bidirectionally through the central repository — each workspace is an
isolated view, but they all share the same underlying history.
