
# Use cases

### Partial cloning

Reduce scope and size of clones by treating subdirectories of the monorepo
as individual repositories.

```
$ git clone http://josh/monorepo.git:/path/to/library.git
```

The partial repo will act as a normal git repository but only contain the files
found in the subdirectory and only commits affecting those files.
The partial repo supports both fetch as well as push operation.

This helps not just to improve performace on the client due to having fewer files in
the tree,
it also enables collaboration on parts of the monorepo with other parties
utilizing git's normal distributed development features.
For example, this makes it easy to mirror just selected parts of your
repo to public github repositories or specific customers.

### Project composition / Workspaces

Simplify code sharing and dependency management. Beyond just subdirectories,
Josh supports filtering, re-mapping and composition of arbitrary virtual repositories
from the content found in the monorepo.

The mapping itself is also stored in the repository and therefore versioned alongside
the code.

Multiple projects, depending on a shared set of libraries, can thus live together in a single repository.
This approach is commonly referred to as “monorepo”, and was popularized by
[Google](https://people.engr.ncsu.edu/ermurph3/papers/seip18.pdf), Facebook or Twitter to name a
few.

In this example, two projects (`project1` and `project2`) coexist in the `central` monorepo.

<table>
    <thead>
        <tr>
            <th>Central monorepo</th>
            <th>Project workspaces</th>
            <th>workspace.josh file</th>
        </tr>
    </thead>
    <tbody>
        <tr>
            <td rowspan=2><img src="./img/central.svg?sanitize=true" alt="Folders and files in central.git" /></td>
            <td><img src="./img/project1.svg?sanitize=true" alt="Folders and files in project1.git" /></td>
            <td>
<pre>
dependencies = :/modules:[
    ::tools/
    ::library1/
]
</pre>
        </tr>
        <tr>
            <td><img src="./img/project2.svg?sanitize=true" alt="Folders and files in project2.git" /></td>
            <td>
<pre>libs/library1 = :/modules/library1</pre></td>
        </tr>
    </tbody>
</table>

Workspaces act as normal git repos:

```
$ git clone http://josh/central.git:workspace=workspaces/project1.git
```

Each of the subprojects defines a `workspace.josh` file, defining the mapping between the original central.git repository and the hierarchy in use inside of the project.

In this setup, project1 and project2 can seemlessly depend on the latest version of library1, while only checking out the part of the central monorepo that's needed for their purpose.
What's more, any changes to a shared module will be synced in both directions.

If a developer of the library1 pushed a new update, both projects will get the new version, and the developer will be able to check if they broke any test.
If a developer of project1 needs to update the library, the changes will be automatically shared back into central, and project2.

### Simplified CI/CD

With everything stored in one repo, CI/CD systems only need to look into one source for each particular
deliverable.
However in traditional monorepo environments dependency mangement is handled by the build system.
Build systems are usually taylored to specific languages and need their input already checked
out on the filesystem.
So the question:

> "What deliverables are affected by a given commit and need to be rebuild?"

cannot be answered without cloning the entire repository and understanding how the languages
used handle dependencies.

In particular when using C family languages, hidden dependencies on header files are easy to miss.
For this reason limiting the visibility of files to the compiler by sandboxing is pretty much a requirement
for reproducible builds.

With Josh, each deliverable gets its own virtual git repository with dependencies declared in the `workspace.josh`
file. This means answering the above question becomes as simple as comparing commit ids.
Furthermore due to the tree filtering each build is guaranteed to be perfectly sandboxed
and only sees those parts of the monorepo that have actually been mapped.

This also means the deliverables to be re-built can be determined without cloning any repos like
typically necessary with normal build tools.

### GraphQL API

It is often desireable to access content stored in git without requiring a clone of the repository.
This is usefull for CI/CD systems or web frontends such as dashboards.

Josh exposes a GraphQL API for that purpose. For example, it can be used to find all workspaces currently
present in the tree:

```
query {
  rev(at:"refs/heads/master", filter:"::**/workspace.josh") {
    files { path }
  }
}
```

### Caching proxy

Even without using the more advanced features like partial cloning or workspaces,
`josh-proxy` can act as a cache to reduce traffic between locations or keep your CI from
performing many requests to the main git host.

