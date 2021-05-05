# Getting Started

This book will guide you into setting up the josh
proxy to serve your own git repository.

> ***NOTE***
> 
> All the commands are included from the file `gettingstarted.t`
> which can be run with [cram](https://bitheap.org/cram/).

## Setting up the proxy

Josh is distributed via [docker hub](https://hub.docker.com/r/esrlabs/josh-proxy),
and is installed and started with the following command:

```shell
{{#include gettingstarted.t:docker_github}}
```

This starts josh as a proxy to github.com, in a docker container, 
mounting the ./git\_data folder to the image for use by josh.

## Cloning a repository

Once josh is running, we can clone a repository through it.
For example, let's clone josh:

```shell
{{#include gettingstarted.t:clone_full}}
```

As we can see, this repository is simply the normal josh one:

```shell
{{#include gettingstarted.t:ls_full}}
```

## Extracting a module

Josh becomes interesting when we want to extract a module.
Let's check out the josh repository again, but this time let's filter
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
