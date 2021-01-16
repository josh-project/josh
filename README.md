[![Build Status](https://github.com/esrlabs/josh/workflows/Rust/badge.svg?branch=master)](https://github.com/esrlabs/josh/actions)
[![Documentation Status](https://readthedocs.org/projects/josh/badge/?version=latest)](https://josh.readthedocs.io/en/latest/?badge=latest)

Just One Single History
=======================

Josh combines the advantages of mono-repo with those of many-repos by leveraging a blazingly-fast, incremental and reversible implementation of git history filtering.

The [documentation](https://josh.readthedocs.io/en/latest/index.html) describes the filtering mechanism, as well as the tools provided by Josh: the josh library, `josh-proxy` and `josh-filter`.

Concept
-------

Traditionally history filtering has been viewed as an expensive operation that should only be done as a means to fix issues with a repository or as part of a migration to a different repository structure, like switching from many-repos to mono-repo or vice versa, purging big binary files or removing accidentally committed secrets.

The implementation shipped with git (`git-filter branch`) is only usable as a once in a lifetime last resort for anything but tiny repositories.

Faster versions of history filtering have been implemented (like [git-filter-repo](https://github.com/newren/git-filter-repo) or the [BFG repo cleaner](https://rtyley.github.io/bfg-repo-cleaner/)) but those, while much faster, are also designed as tools for doing occasional, destructive maintenance tasks. Usually with the idea already in mind that once the filtering is complete the old history should be discarded.

The idea behind `josh` starts with two questions: 
What if history filtering could be so fast that it can be part of a normal everyday workflow, running on every single push and fetch in milliseconds time without the user even noticing?
And what if history filtering was a non-destructive, reversible operation?

Under those two premises a filter operation stops being a maintenance task. It seamlessly relates histories between repos, which can be used by developers and CI systems interchangeably in whatever way is most suitable to the task at hand.

How is this possible?

Filtering history is a highly predictable task: The set of filters that tend to be used for any given repository is limited. The input to the filter, a git branch, only gets modified in an incremental way.
So by keeping a persistent cache between filter runs, the work needed to re-run a filter on a new commit (and its history) becomes proportional to the number of changes since the last run.
Thus, it no longer depends on the total length of the history. And for most filters, also not on the size of the trees.

So what has long been known to be true for performing merges also applies to history filtering: The more often it is done the less work it takes each time.

To guarantee filters to be reversible we have to restrict the kind of filter that can be used. So it is not possible to write arbitrary filters using a scripting language like it is allowed in other tools.
To still be able to cover a wide range of use cases we introduce a domain specific language to express more complex filters as a combination of simpler ones. Apart from guaranteeing reversibility, the use of a DSL also enables pre-optimization of filter expressions to minimize both the amount of work to be done to execute the filter as well as the on-disk size of the persistent cache.

Use cases
---------
#### Workspaces in a mono-repo
Multiple projects, depending on a shared set of libraries, can live together in a single repository. This approach is commonly referred to as “monorepo”, and was popularized by [Google](https://people.engr.ncsu.edu/ermurph3/papers/seip18.pdf), Facebook or Twitter to name a few.

In this example, two projects (`project1` and `project2`) coexist in the `central` monorepo.

<table>
    <thead>
        <tr>
            <th>Central monorepo</th>
            <th>Project workspaces</th>
            <th>workspace.josh files</th>
        </tr>
    </thead>
    <tbody>
        <tr>
            <td rowspan=2><img src="docs/img/central.svg?sanitize=true" alt="Folders and files in central.git" /></td>
            <td><img src="docs/img/project1.svg?sanitize=true" alt="Folders and files in project1.git" /></td>
            <td><tt>dependencies/tools = :/modules/tools</tt>
            <br /> <tt>dependencies/library1 = :/modules/library1</tt></td>
        </tr>
        <tr>
            <td><img src="docs/img/project2.svg?sanitize=true" alt="Folders and files in project2.git" /></td>
            <td><tt>libs/library1 = :/modules/library1</tt></td>
        </tr>
    </tbody>
</table>

Each of the subprojects defines a workspace.josh file, defining the mapping between the original central.git repository and the hierarchy in use inside of the project.

In this setup, project1 and project2 can seemlessly depend on the latest version of library1, while only checking out the part of the central monorepo that's needed for their purpose.
What's more, any changes to a shared module will be synced in both directions.

If a developper of the library1 pushed a new update, both projects will get the new version, and the developper will be able to check if they broke any test.
If a developper of project1 needs to update the library, the changes will be automatically shared back into central, and project2.

=========================================

From Linus Torvalds 2007 talk at Google about git:

Audience:

Can you have just a part of files pulled out of a repository, not the entire repository?

Linus:

You can export things as tarballs, you can export things as individual files, you can rewrite the whole history to say "I want a new version of that repository that only contains that part", you can do that, it is a fairly expensive operation it's something you would do for example when you import an old repository into a one huge git repository and then you can split it later on to be multiple smaller ones, you can do it, what I am trying to say is that you should generally try to avoid it. It's not that git can not handle huge projects, git would not perform as well as it would otherwise. And you will have issues that you wish you didn't not have.

So I am skipping this issue and going back to the performance issue. One of the things I want to say about performance is that a lot of people seem to think that performance is about doing the same thing, just doing it faster, and that is not true.

That is not what performance is all about. If you can do something really fast, really well, people will start using it differently.
 
