# Working with workspaces

Josh really starts to shine when using workspaces.

Simply put, a workspace is a list of files and folders remapped from a central repository
into a new repository layout. For example, a shared library can be mapped into multiple
workspaces, each placing it at the path that makes sense for that project.

In this chapter we'll set up a small monorepo with two libraries and two applications,
then use workspaces to develop against them in isolation.

> ***NOTE***
>
> All the commands are included from the file `workspaces.t`
> which can be run with [cram](https://bitheap.org/cram/).

## Test set-up

> ***NOTE***
>
> The following section sets up a local git server with made-up content for the sake of
> this tutorial. You're free to follow along, or skip to the next section if you already
> have a repository to work with.

We need an upstream git repository to work with. For this tutorial we'll create a bare
repository and serve it over HTTP using the
[hyper_cgi](https://crates.io/crates/hyper_cgi) test server:

```shell
{{#include workspaces.t:git_setup}}
```

```shell
{{#include workspaces.t:git_server}}
```

Our server is ready, serving all the repos in the `remote` folder on port `8001`.

```shell
{{#include workspaces.t:clone}}
```

### Adding some content

The repository is empty, so let's populate it. The
[populate.sh](populate.sh) script creates two libraries and two applications that use
them:

```shell
{{#include workspaces.t:populate}}
```

## Creating our first workspace

To facilitate development on application1 we want a workspace scoped to it. A workspace
is cloned using `josh clone` with the `:workspace=` filter, pointing at the directory
inside the monorepo that contains (or will contain) the `workspace.josh` file:

```shell
{{#include workspaces.t:clone_workspace}}
```

Looking inside the cloned workspace we see only the files and history relevant to
application1.

> ***NOTE***
>
> Josh allows us to create a workspace out of any directory, even one that doesn't
> exist yet.

### Adding workspace.josh

The `workspace.josh` file describes how folders from the central repository should be
mapped into this workspace.

Since application1 depends on library1, let's add it:

```shell
{{#include workspaces.t:library_ws}}
```

We can now push the changes and pull the updated workspace:

```shell
{{#include workspaces.t:library_sync}}
```

Let's observe the result:

```shell
{{#include workspaces.t:library_sync2}}
```

After pushing and pulling, the mapped library has appeared in the workspace.

One surprising thing is the history: our "mapping" commit became a merge commit. This is
because Josh needs to merge the history of the mapped module into the workspace
repository. After this, all commits will be present in both histories.

By the way, what does the history look like in the central repository?

```shell
{{#include workspaces.t:real_repo}}
```

We can see the newly added `workspace.josh` commit in application1's directory, and as
expected, no merge commit there.

### Interacting with workspaces

Let's create a second workspace for application2, which depends on both libraries:

```shell
{{#include workspaces.t:application2}}
```

Syncing as before:

```shell
{{#include workspaces.t:app2_sync}}
```

Our local folder now contains all the requested files:

```shell
{{#include workspaces.t:app2_files}}
```

And the history includes the history of both libraries:

```shell
{{#include workspaces.t:app2_hist}}
```

Note that since we created the workspace and added both dependencies in a single commit,
the history contains just that one merge commit.

#### Pushing a change from a workspace

While testing application2 we noticed a typo in the `library1` dependency. Let's fix
it:

```shell
{{#include workspaces.t:fix_typo}}
```

We can push this change like any normal git commit:

```shell
{{#include workspaces.t:push_change}}
```

Since the change was pushed back to the central repository, a developer working in the
application1 workspace can now pull it:

```shell
{{#include workspaces.t:app1_pull}}
```

The fix has been propagated:

```shell
{{#include workspaces.t:app1_log}}
```
