# Working with workspaces

> ***NOTE***
>
> All the commands are included from the file `workspaces.t`
> which can be run with [cram](https://bitheap.org/cram/).

Josh really starts to shine when using workspaces.

Simply put, they are a list of files and folders, remapped from the central repository
to a new repository.
For example, a shared library could be used by various workspaces, each mapping it to
their appropriate subdirectory.

In this chapter, we're going to set up a new git repository with a couple of libraries,
and then use it to demonstrate the use of workspaces.

## Test set-up

> ***NOTE***
>
> The following section describes how to set-up a local git server with made-up content
> for the sake of this tutorial.
> You're free to follow it, or to use your own existing repository, in which case you
> can skip to the next section

To host the repository for this test, we need a git server.
We're going to run git as a [cgi](https://en.wikipedia.org/wiki/Common_Gateway_Interface)
program using its provided http backend, served with the test server included in
the [hyper\_cgi](https://crates.io/crates/hyper_cgi) crate.

### Serving the git repo
First, we create a *bare* repository, which will be served by hyper\_cgi. We enable
the option `http.receivepack` to allow the use of `git push` from the clients.

```shell
{{#include workspaces.t:git_setup}}
```

Then we start the server which will allow clients to access the repository through
http.

```shell
{{#include workspaces.t:git_server}}
```

Our server is ready, serving all the repos in the `remote` folder on port `8001`.

```shell
{{#include workspaces.t:clone}}
```

### Adding some content
Of course, the repository is for now empty, and we need to populate it.
The [populate.sh](populate.sh) script creates a couple of libraries, as well as two applications that use
them.

```shell
{{#include workspaces.t:populate}}
```

## Creating our first workspace
Now that we have a git repo populated with content, let's serve it through josh:

```shell
{{#include workspaces.t:docker_josh}}
```

> ***NOTE***
>
> For the sake of this example, we run docker with --network="host" instead of publishing the port.
> This is so that docker can access localhost, where our ad-hoc git repository is served.

To facilitate developement on applications 1 and 2, we want to create workspaces for them.
Creating a new workspace looks very similar to checking out a subfolder through josh, as explained
in "Getting Started".

Instead of just the name of the subfolder, though, we also use the `:workspace=` filter:

```shell
{{#include workspaces.t:clone_workspace}}
```

Looking into the newly cloned workspace, we see our expected files and the history containing the
only relevant commit.

> ***NOTE***
>
> Josh allows us to create a workspace out of any directory, even one that doesn't exist yet.

### Adding workspace.josh

The workspace.josh file describes how folders from the central repository (real\_repo.git)
should be mapped to the workspace repository.

Since we depend on library1, let's add it to the workspace file.

```shell
{{#include workspaces.t:library_ws}}
```

We decided to map library1 to modules/lib1 in the workspace.
We can now sync up with the server:

```shell
{{#include workspaces.t:library_sync}}
```

let's observe the result:

```shell
{{#include workspaces.t:library_sync2}}
```

After pushing and fetching the result, we see that it has been succesfully mapped by josh.

One suprising thing is the history: our "mapping" commit became a merge commit!
This is because josh needs to merge the history of the module we want to map into the
repository of the workspace.
After this is done, all commits will be present in both of the histories.

> ***NOTE***
>
> `git sync` is a utility provided with josh which will push contents, and, if josh tells
> it to, fetch the transformed result. Otherwise, it works like git push.

By the way, what does the history look like on the real\_repo ?

```shell
{{#include workspaces.t:real_repo}}
```

We can see the newly added commit for workspace.josh in application1, and as expected,
no merge here.

### Interacting with workspaces

Let's now create a second workspce, this time for application2.
It depends on library1 and library2.

```shell
{{#include workspaces.t:application2}}
```

Syncing as before:

```shell
{{#include workspaces.t:app2_sync}}
```

And our local folder now contains all the files requested:

```shell
{{#include workspaces.t:app2_files}}
```

And the history includes the history of both of the libraries:

```shell
{{#include workspaces.t:app2_hist}}
```

Note that since we created the workspace and added the dependencies in one single commit,
the history just contains this one single merge commit.

#### Pushing a change from a workspace

While testing application2, we noticed a typo in the `library1` dependency.
Let's go ahead a fix it!

```shell
{{#include workspaces.t:fix_typo}}
```

We can push this change like any normal git change:

```shell
{{#include workspaces.t:push_change}}
```

Since the change was merged in the central repository, 
a developer can now pull from the application1 workspace.

```shell
{{#include workspaces.t:app1_pull}}
```

The change has been propagated!

```shell
{{#include workspaces.t:app1_log}}
```
