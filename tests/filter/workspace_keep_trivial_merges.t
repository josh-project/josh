  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

Create initial state: two directories (a/ and b/) plus a workspace that only covers a/

  $ mkdir a b ws
  $ echo contents1 > a/file1
  $ echo contents2 > b/file2
  $ cat > ws/workspace.josh <<EOF
  > :/a
  > EOF
  $ git add .
  $ git commit -m "initial" 1> /dev/null

Create a branch that only touches a/ — this is key: the merge will be trivial in b/ view

  $ git checkout -b branch1
  Switched to a new branch 'branch1'
  $ echo branch_content > a/file_branch
  $ git add .
  $ git commit -m "branch1: add a/file_branch" 1> /dev/null

Back on master, add a change only to b/

  $ git checkout master
  Switched to branch 'master'
  $ echo master_content > b/file_master
  $ git add .
  $ git commit -m "master: add b/file_master" 1> /dev/null

Merge branch1 into master.
The resulting merge commit is TRIVIAL in the b/ view because branch1 only touched a/:

  $ git merge -q branch1 --no-ff
  $ git log --graph --pretty=%s
  *   Merge branch 'branch1'
  |\  
  | * branch1: add a/file_branch
  * | master: add b/file_master
  |/  
  * initial

Extend the workspace to include b/ as well

  $ cat > ws/workspace.josh <<EOF
  > :[
  >   :/a
  >   :/b
  > ]
  > EOF
  $ git add .
  $ git commit -m "extend workspace" 1> /dev/null

Without keep-trivial-merges: the trivial merge in b/ (extra-parent history) is dropped

  $ josh-filter -s ":workspace=ws"
  e90832787df33830788ecf12bbd4a2fd2750ddb5
  [3] :/b
  [4] :workspace=ws
  [5] reachable_roots
  [5] sequence_number
  $ git log --graph --pretty=%s FILTERED_HEAD
  *   extend workspace
  |\  
  | * master: add b/file_master
  | * initial
  *   Merge branch 'branch1'
  |\  
  | * branch1: add a/file_branch
  |/  
  * initial

With keep-trivial-merges: the trivial merge is preserved in b/'s extra-parent history

  $ josh-filter -s ":~(history=\"keep-trivial-merges\")[:workspace=ws]"
  625c122538bba32a02e172460e1e02d9d0c8ea3e
  [3] :/b
  [3] :~(
      history="keep-trivial-merges"
  )[
      :/b
  ]
  [4] :workspace=ws
  [4] :~(
      history="keep-trivial-merges"
  )[
      :workspace=ws
  ]
  [5] reachable_roots
  [5] sequence_number
  $ git log --graph --pretty=%s FILTERED_HEAD
  *   extend workspace
  |\  
  | *   Merge branch 'branch1'
  | |\  
  | * | master: add b/file_master
  | |/  
  | * initial
  *   Merge branch 'branch1'
  |\  
  | * branch1: add a/file_branch
  |/  
  * initial
