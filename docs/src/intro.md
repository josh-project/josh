![Just One Single History](https://raw.githubusercontent.com/josh-project/josh/master/splash.png)

Josh combines the advantages of monorepos with those of multirepos by leveraging a blazingly-fast,
incremental, and reversible implementation of git history filtering.

## Concept

Traditionally, history filtering has been viewed as an expensive operation that should only be
performed to fix issues with a repository, such as purging big binary files or removing
accidentally-committed secrets, or as part of a migration to a different repository structure, like
switching from multirepo to monorepo (or vice versa).

The implementation shipped with git (`git-filter branch`) is only usable as a once-in-a-lifetime
last resort for anything but tiny repositories.

Faster versions of history filtering have been implemented, such as
[git-filter-repo](https://github.com/newren/git-filter-repo) or the
[BFG repo cleaner](https://rtyley.github.io/bfg-repo-cleaner/). Those, while much faster, are
designed for doing occasional, destructive maintenance tasks, usually with the idea already in mind
that once the filtering is complete the old history should be discarded.

The idea behind `josh` started with two questions:

1. What if history filtering could be so fast that it can be part of a normal, everyday workflow,
   running on every single push and fetch without the user even noticing?
2. What if history filtering was a non-destructive, reversible operation?

Under those two premises a filter operation stops being a maintenance task. It seamlessly relates
histories between repos, which can be used by developers and CI systems interchangeably in whatever
way is most suitable to the task at hand.

How is this possible?

Filtering history is a highly predictable task: The set of filters that tend to be used for any
given repository is limited, such that the input to the filter (a git branch) only gets modified in
an incremental way. Thus, by keeping a persistent cache between filter runs, the work needed to
re-run a filter on a new commit (and its history) becomes proportional to the number of changes
since the last run; The work to filter no longer depends on the total length of the history.
Additionally, most filters also do not depend on the size of the trees.

What has long been known to be true for performing merges also applies to history filtering: The
more often it is done the less work it takes each time.

To guarantee filters are reversible we have to restrict the kind of filter that can be used; It is
not possible to write arbitrary filters using a scripting language like is allowed in other tools.
To still be able to cover a wide range of use cases we have introduced a domain-specific language to
express more complex filters as a combination of simpler ones. Apart from guaranteeing
reversibility, the use of a DSL also enables pre-optimization of filter expressions to minimize both
the amount of work to be done to execute the filter as well as the on-disk size of the persistent
cache.



>*_From Linus Torvalds 2007 talk at Google about git:_*
>
>**Audience:**
>
>Can you have just a part of files pulled out of a repository, not the entire repository?
>
>**Linus:**
>
>You can export things as tarballs, you can export things as individual files, you can rewrite the
>whole history to say "I want a new version of that repository that only contains that part", you
>can do that, it is a fairly expensive operation it's something you would do for example when you
>import an old repository into a one huge git repository and then you can split it later on to be
>multiple smaller ones, you can do it, what I am trying to say is that you should generally try to
>avoid it. It's not that git cannot handle huge projects, git would not perform as well as it would
>otherwise. And you will have issues that you wish you didn't not have.
>
>So I am skipping this issue and going back to the performance issue. One of the things I want to
>say about performance is that a lot of people seem to think that performance is about doing the
>same thing, just doing it faster, and that is not true.
>
>That is not what performance is all about. If you can do something really fast, really well, people
>will start using it differently.
> 
