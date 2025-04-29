# Frequently Asked Questions

## How is Josh different from git sparse-checkout?

Josh operates on the git object graph and is unrelated to checking out files and the working tree on the filesystem, which is the only thing sparse-checkout is concerned with. A sparse checkout does not influence the contents of the object database and also not what gets downloaded over the network.
Both can certainly be used together if needed.

## How is Josh different from partial clone?

A partial clone will cause git to download only parts of an object database according to some predicate. It is still the same object database with the history having the same commits and sha1s. It still allows loading skipped parts of the object database at a later point.
Josh creates an alternate history that has no reference to the skipped parts. It is as such very similar to git filter-branch just faster, with added features and a different user interface.

## How is it different from submodules?

Where git submodules are multiple, independant repos, referencing each other with SHAs, Josh supports the monorepo approach.
All of the code is in one single repo which can easily be kept in sync, and Josh provides any sub folder (or in the case of workspaces, more complicated recombination of folders) as their own git repository.
These repos are transparently synchronised both ways with the main monorepo.
Josh can thus do more than submodules can, and is easier and faster to use.

## How is it different from `git subtree`?

The basic idea behind Josh is quite similar to `git subtree`. However `git subtree`, just like `git filter-branch`, is way too slow for everyday use, even on medium sized repos.
`git subtree` can only achieve acceptable performance when squashing commits and therefore losing history. One core part of Josh is essentially a much faster implementation
of `git subtree split` which has been specifically optimized for being run frequently inside the same repository.


## How is Josh different from `git filter-repo`?

Both  `josh-filter` as well as `git filter-repo` enable very fast rewriting of Git history and thus can in simple cases be used
for the same purpose.

Which one is right in more advanced use cases depends on your goals: `git filter-repo` offers more flexibility and options
on what kind of filtering it supports, like rewriting commit messages or even plugging arbitrary scripts into the filtering.

Josh uses a DSL instead of arbitrary scripts for complex filters and is much more restrictive in the kind of filtering
possilbe, but in exchange for those limitations offers incremental filtering as well as bidirectional operation, meaning converting changes between both the original and the filtered repos.

## How is Josh different from all of the above alternatives?

Josh includes `josh-proxy` which offers repo filtering as a service, mainly intended to support monorepo workflows.