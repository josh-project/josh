
# Use cases

## Workspaces in a mono-repo

Multiple projects, depending on a shared set of libraries, can live together in a single repository.
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

Each of the subprojects defines a `workspace.josh` file, defining the mapping between the original central.git repository and the hierarchy in use inside of the project.

In this setup, project1 and project2 can seemlessly depend on the latest version of library1, while only checking out the part of the central monorepo that's needed for their purpose.
What's more, any changes to a shared module will be synced in both directions.

If a developper of the library1 pushed a new update, both projects will get the new version, and the developper will be able to check if they broke any test.
If a developper of project1 needs to update the library, the changes will be automatically shared back into central, and project2.
