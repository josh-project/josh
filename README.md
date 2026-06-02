![Just One Single History](/splash.jpg)

Josh – “Just One Single History” – is a collection of tools and services together composing
a platform for scaling out distributed development and collaboration with Git.

Our goal is to address challenges that arise when the number of people working on software
within any given organization or even across multiple organizations grows. The experience we
aim for is: work in a codebase of any size, with any number of contributors, without slowing
down the change velocity.

## Use cases

### Repo filtering

One of the most common pain points software organizations encounter is the need to
split their repositories into multiple per-project repos in order to address scalability,
access control, and scope issues.

We believe that having multiple repositories is not inherently advantageous by itself,
but rather is a stopgap solution driven by inefficiencies of the currently available tooling.
Over the years, Git platforms like GitHub have created assumptions in the collective consciousness
about what is and is not possible in Git, and to this day, this influences decisions
organizations make about the way they develop software.

Josh implements the core component that challenges these assumptions: fast, reversible
Git history transformation enables us to not think about Git repository boundaries anymore:
they disappear as soon as we can present any part of a Git repository independently,
preserving history and ability to contribute. This enabled unprecedented flexibility:
instead of static boundaries – which often reflect organizational boundaries and
create silos – the boundaries can be decided in the moment, for any given context,
thousands of times per second.

For example, Josh enables you to represent a subfolder of your monorepo as an independent
repository, limiting visibility, scope, CI rebuilds, etc.

You can try this out quickly with this repo and the `docs` folder:

```
git clone https://josh-project.dev/josh.git:/docs.git
```

### Build / CI

Another common use case for Josh is integration with CI and build systems.
Josh offers a GraphQL API that allows you to check the state of the repository –
and any partial projections within it – without actually checking out the repo.
This provides a way to quickly answer the question:

>_Has this code already been successfully built before?_

for any subproject within the repo.

## Getting started

Today, the Josh project ships two major components:

* _josh CLI_: local tool that enables working with partial projections of Git repos. See
  <a href="https://josh-project.github.io/josh/guide/gettingstarted.html">CLI getting started guide</a>.
* _josh-proxy_: Git HTTP and SSH proxy that provides on-the-fly transformation of history
  for multiple users with shared cache, as well as GraphQL APIs. See
  <a href="https://josh-project.github.io/josh/reference/proxy.html">proxy documentation</a>.

Apart from the links above, there are a couple of core concepts that Josh relies on:

* **Filters**: a way of describing the desired repository transformation: https://josh-project.github.io/josh/reference/filters.html
* **Workspaces**: persisted, versioned filters https://josh-project.github.io/josh/guide/workspaces.html

## FAQ

See the [FAQ](https://josh-project.github.io/josh/faq.html).

## Upcoming features

We are working on a number of features that aren’t yet fully mature, but will eventually be
available in stable releases. Those include:

* Merge queue with support for filtering and PR stacks
* Code review UI for stacked changes
* Filters written in Starlark syntax
* `josh compose` – containerized build orchestrator built on top of the concept of filters

<hr/>

<hr/>

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
>otherwise. And you will have issues that you wish you didn't have.
>
>So I am skipping this issue and going back to the performance issue. One of the things I want to
>say about performance is that a lot of people seem to think that performance is about doing the
>same thing, just doing it faster, and that is not true.
>
>That is not what performance is all about. If you can do something really fast, really well, people
>will start using it differently.
>
