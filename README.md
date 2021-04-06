![Just One Single History](/banner.png)

[![Build Status](https://github.com/esrlabs/josh/workflows/Rust/badge.svg?branch=master)](https://github.com/esrlabs/josh/actions)

Combine the advantages of a monorepo with those of multirepo setups by leveraging a
blazingly-fast, incremental, and reversible implementation of git history filtering.

It acts as a proxy and can be integrated with any http based git host.

## Use cases

### Partial cloning

Reduce load on the network and client machines by cloning subdirectories of the monorepo
as individual repositories.

```
$ git clone http://josh/monorepo.git/path/to/library.git
```

The partial repo will act as a normal git repository but only contain the files
found in the subdirectory and only commits affecting those files.
The partial repo supports both fetch as well as push operation.

### Caching proxy

Even without using the more advanced features like partial cloning `josh-proxy` can
act as a cache to reduce traffic between locations or keep your CI from
doing lot's of requests to the main git host.

### Project composition / Workspaces

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

```
$ git clone http://josh/monorepo.git:workspace=workspaces/project1.git
```


