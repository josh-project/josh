# Frequently Asked Questions

## How is josh different from git sparse-checkout?

Josh operates on the git object graph and is unrelated to checking out files and the working tree on the filesystem, which is the only thing sparse-checkout is concerned with. A sparse checkout does not influence the contents of the object database and also not what gets downloaded over the network.
Both can certainly be used together if needed.

## How is josh different from partial clone?

A partial clone will cause git to download only parts of an object database according to some predicate. It is still the same object database with the history having the same commits and sha1s. It still allows loading skipped parts of the object database at a later point.
Josh creates an alternate history that has no reference to the skipped parts. It is as such very similar to git filter-branch just faster, with added features and a different user interface.

## How is it different from submodules?

Where git submodules are multiple, independant repos, referencing each other with SHAs, josh supports the monorepo approach.
All of the code is in one single repo which can easily be kept in sync, and josh provides any sub folder (or in the case of workspaces, more complicated recombination of folders) as their own git repository.
These repos are transparently synchronised both ways with the main monorepo.
Josh can thus do more than submodules can, and is easier and faster to use.

## How is it different from `git subtree`?

The basic idea behind Josh is quite similar to `git subtree`. However `git subtree`, just like `git filter-branch`, is way to slow for everyday use, even on medium sized repos.
`git subtree` can only archieve acceptable performance when squashing commits and therefore loosing history. One core part of Josh is essentially a much faster implementation
of `git subtree split` which has been specifically optimized for being run frequently inside the same repository.

