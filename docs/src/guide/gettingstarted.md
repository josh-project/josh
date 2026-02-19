# Getting Started

This book will guide you into setting up the josh
proxy to serve your own git repository.

> ***NOTE***
> 
> All the commands are included from the file `gettingstarted.t`
> which can be run with [cram](https://bitheap.org/cram/).

## Setting up the proxy

Josh is distributed via [Docker Hub](https://hub.docker.com/r/joshproject/josh-proxy),
and is installed and started with the following command:

```shell
{{#include gettingstarted.t:docker_github}}
```

This starts Josh as a proxy to `github.com`, in a Docker container, 
creating a volume `josh-vol` and mounting it to the image for use by Josh.

## Cloning a repository

Once Josh is running, we can clone a repository through it.
For example, let's clone Josh:

```shell
{{#include gettingstarted.t:clone_full}}
```

As we can see, this repository is simply the normal Josh one:

```shell
{{#include gettingstarted.t:ls_full}}
```

## Cloning a part of the repo

Josh becomes interesting when we want to clone a part of the repo.
Let's check out the Josh repository again, but this time let's filter
only the documentation out:

```shell
{{#include gettingstarted.t:clone_doc}}
```

Note the addition of `:/docs` at the end of the url.
This is called a filter, and it instructs josh to only check out the
given folder.

Looking inside the repository, we now see that the history is quite
different. Indeed, it contains only the commits pertaining to the 
subfolder that we checked out.

```shell
{{#include gettingstarted.t:ls_doc}}
```

This repository is a real repository in which we can pull, commit, push,
as with a regular one. Josh will take care of synchronizing it with
the main one in a transparent fashion.
