# Importing projects

When moving to a monorepo setup, especially in existing organisations,
it is common that the need to consolidate existing project
repositories arises.

The simplest possible case is one where the previous history of a
project does not need to be retained. In this case, the projects files
can simply be copied into the monoreop at the appropriate location and
committed.

If history should be retained, josh can be used for importing a
project as an alternative to built-in git commands like [`git
subtree`][subtree].

Josh's [filter capability](../reference/filters.md) lets you perform
transformations on the history of a git repository to arbitrarily
(re-)compose paths inside of a repository.

A key aspect of this functionality is that all transformations are
*reversible*. This means that if you apply a transformation moving
files from the root of a repository to, say, `tools/project-b`,
followed by an *inverse* transformation moving files from
`tools/project-b` back to the root, you receive the same commit hashes
you put in.

We can use this feature to import a project into our monorepo while
allowing external users to keep pulling on top of the same git history
they already have, just with a new git remote.

There are multiple ways of doing this, with the most common ones
outlined below. You can look at [`josh#596`][import-issue] for a
discussion of several other methods.

### Import with `josh-filter`

Currently, the easiest way to do this is by using the `josh-filter`
binary which is a command-line frontend to josh's filter capabilities.

Inside of our target repository, it would work like this:

1. Fetch the repository we want to import (say, "Project B", from
   `$REPO_URL`).

   ```sh
   $ git fetch $REPO_URL master
   ```

   This will set the `FETCH_HEAD` reference to the fetched repository.

2. Rewrite the history of that repository through josh to make it look
   as if the project had always been developed at our target path
   (say, `tools/project-b`).


   ```sh
   $ josh-filter ':prefix=tools/project-b' FETCH_HEAD
   ```

   This will set the `FILTERED_HEAD` reference to the rewritten
   history.

3. Merge the rewritten history into our target repository.

   ```sh
   $ git merge --allow-unrelated FILTERED_HEAD
   ```

   After this merge commit, the previously external project now lives
   at `tools/project-b` as expected.

4. Any external users can now use the `:/tools/project-b` josh filter
   to retrieve changes made in the new project location - without the
   git hashes of their existing commits changing (that is to say,
   without conflicting).

### Import by pushing to josh

If your monorepo is already running a `josh-proxy` in front of it, you
can also import a project by pushing a project merge to josh.

This has the benefit of not needing to clone the entire monorepo
locally to do a merge, but the drawback of using a different, slightly
slower filter mechanism when exporting the tree back out. For projects
with very large history, consider using the `josh-filter` mechanism
outlined above.

Pushing a project merge to josh works like this:

1. Assume we have a local checkout of "Project B", and we want to
   merge this into our monorepo. There is a `josh-proxy` running at
   `https://git.company.name/monorepo.git`. We want to merge this
   project into `/tools/project-b` in the monorepo.

2. In the checkout of "Project B", add the josh remote:

   ```sh
   git remote add josh https://git.company.name/monorepo.git:/tools/project-b.git
   ```

   Note the use of the `/tools/project-b.git` josh filter, which
   points to a path that should not yet exist in the monorepo.

3. Push the repository to josh with the `-o merge` option, creating a
   merge commit introducing the project history at that location,
   while retaining its history:

   ```sh
   git push josh $ref -o merge
   ```

### Note for Gerrit users

With either method, when merging a set of new commits into a Gerrit
repository and going through the standard code-review process, Gerrit
might complain about missing Change-IDs in the imported commits.

To work around this, the commits need to first be made "known" to
Gerrit. This can be achieved by pushing the new parent of the merge
commit to a separate branch in Gerrit directly (without going through
the review mechanism). After this Gerrit will accept merge commits
referencing that parent, as long as the merge commit itself has a
Change-ID.

Some monorepo setups on Gerrit use a special unrestricted branch like
`merge-staging` for this, to which users with permission to import
projects can force-push unrelated histories.

[subtree]: https://manpages.debian.org/testing/git-man/git-subtree.1.en.html
[import-issue]: https://github.com/josh-project/josh/issues/596
