  $ export TESTTMP=${PWD}
  $ git init -q repo
  $ cd repo
  $ mkdir -p apps/frontend libs/shared
  $ echo app > apps/frontend/app.txt
  $ echo shared > libs/shared/shared.txt
  $ git add .
  $ git commit -q -m "initial"

Create a workspace definition with repeatable mappings.

  $ josh workspace create workspaces/frontend --map app=:/apps/frontend \
  >     --map libs/shared=:/libs/shared
  Created workspace 'workspaces/frontend'
  Definition: workspaces/frontend/workspace.josh
  
  app = :/apps/frontend
  libs/shared = :/libs/shared

  $ cat workspaces/frontend/workspace.josh
  app = :/apps/frontend
  libs/shared = :/libs/shared

List, show, and validate workspace definitions.

  $ josh workspace list
  valid	workspaces/frontend

  $ josh workspace show workspaces/frontend
  Workspace: workspaces/frontend
  Definition: workspaces/frontend/workspace.josh
  Status: valid
  Filter:
    app = :/apps/frontend
    ::libs/shared/

  $ josh workspace validate
  valid	workspaces/frontend

Existing definitions are protected, and dry-run does not write.

  $ josh workspace create workspaces/frontend 2>&1
  Error: Workspace 'workspaces/frontend' already exists; pass --force to replace it
  [1]

  $ josh workspace create workspaces/backend --map src=:/apps/frontend --dry-run
  Would create workspace 'workspaces/backend'
  Definition: workspaces/backend/workspace.josh
  
  src = :/apps/frontend
  $ test ! -e workspaces/backend/workspace.josh

Checkout previews the current working tree, including the uncommitted definition.

  $ josh workspace checkout workspaces/frontend ../frontend-preview 2>/dev/null
  Checked out workspace 'workspaces/frontend' at ${TESTTMP}/frontend-preview (glob)

  $ tree ../frontend-preview -a -I .git
  ../frontend-preview
  |-- app
  |   `-- app.txt
  |-- libs
  |   `-- shared
  |       `-- shared.txt
  `-- workspace.josh
  
  4 directories, 3 files

Invalid workspaces are visible and make validation fail.

  $ mkdir workspaces/broken
  $ echo 'not a filter =' > workspaces/broken/workspace.josh
  $ josh workspace list
  invalid	workspaces/broken
  valid	workspaces/frontend

  $ josh workspace validate >/dev/null 2>/dev/null
  [1]
