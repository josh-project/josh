![Just One Single History](/banner.png)

[![Build Status](https://github.com/esrlabs/josh/workflows/Rust/badge.svg?branch=master)](https://github.com/esrlabs/josh/actions)

Combine the advantages of a monorepo with those of multirepo setups by leveraging a
blazingly-fast, incremental, and reversible implementation of git history filtering.

`josh-proxy` can be integrated with any http based git host:

```
$ docker run -p 8000:8000 -e JOSH_REMOTE=https://github.com -v josh-vol:/data/git esrlabs/josh-proxy:latest
```

## Use cases

### Partial cloning

Reduce scope and size of clones by treating subdirectories of the monorepo
as individual repositories.

```
$ git clone http://josh/monorepo.git:/path/to/library.git
```

The partial repo will act as a normal git repository but only contain the files
found in the subdirectory and only commits affecting those files.
The partial repo supports both fetch as well as push operation.

This helps not just to improve performace on the client due to less files in
the tree.
It also enables collaboration on parts of the monorepo with other parties
utilizing git's normal distributed development features.
For example this makes it every easy to mirror just selected parts of your
repo to public github or specific customers.

### Project composition / Workspaces

Simplify code sharing and dependency management. Beyond just subdirectories,
Josh supports selecting, re-mapping and compsing of arbitary virtual repositories
from the content found in the monorepo.

The mapping itself is also stored in the repository and there versioned alongside
the code.

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
            <td rowspan=2><img src="docs/src/img/central.svg?sanitize=true" alt="Folders and files in central.git" /></td>
            <td><img src="docs/src/img/project1.svg?sanitize=true" alt="Folders and files in project1.git" /></td>
            <td>
<pre>
dependencies = :/modules:[
    ::tools/
    ::library1/
]
</pre>
        </tr>
        <tr>
            <td><img src="docs/src/img/project2.svg?sanitize=true" alt="Folders and files in project2.git" /></td>
            <td>
<pre>libs/library1 = :/modules/library1</pre></td>
        </tr>
    </tbody>
</table>

Workspaces act as normal git repos:

```
$ git clone http://josh/central.git:workspace=workspaces/project1.git
```

### Simplfied CI/CD

With everything stored in one repo, CI/CD system only need to look into one source for each particular
deliverable.
However building multiple deliverables from from one big tree, introduces a new complexity
into the build system. Now build tools or package managers, typically taylored to specfic languages
or toolchains, need to be used to answer the question: "What deliverables are affected by a given commit
and need to be rebuild?".

With Josh, each deliverable gets it's own virtual git repository with dependencies declared in the `workspace.josh`
file. This means answering above question becomes as simple as comparing commit ids.
Furthermore due to the tree filtering each build is guaranteed to be perfectly sandboxed
and only sees those parts of the monorepo that have actually been mapped.

This also means the deliverables to be re-build can be determined without cloning any repos like
typically necessary with normal build tools.

### Caching proxy

Even without using the more advanced features like partial cloning or workspace,
`josh-proxy` can act as a cache to reduce traffic between locations or keep your CI from
doing lot's of requests to the main git host.


