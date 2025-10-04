  $ export GIT_TREE_FMT='%(objectmode) %(objecttype) %(objectname) %(path)'

  $ export TESTTMP=${PWD}
  $ cd ${TESTTMP}

  $ git init -q repo
  $ cd repo
  $ mkdir -p code

Populate repo contents for the first commit

  $ cat << EOF > code/app.js
  > async fn main() {
  >   await fetch("http://127.0.0.1");
  > }
  > EOF

  $ cat << EOF > code/lib.js
  > fn log() {
  >   console.log("logged!");
  > }
  > EOF

Create a workspace: the :hold filter must be applicable per-commit, so it should
be tied to commit sha1 either via workspace or via hook. Otherwise we are going
to hold a file in every commit, resulting in no versions of the file appearing at all.
Don't hold anything yet.

  $ mkdir -p workspaces/code
  $ cat << EOF > workspaces/code/workspace.josh
  > :/code
  > EOF

  $ git add .
  $ git commit -q -m "first commit"

  $ josh-filter ':workspace=workspaces/code'
  $ git ls-tree --format="${GIT_TREE_FMT}" -r FILTERED_HEAD
  100644 blob 0747fcb9cd688a7876932dcc30006e6ffa9106d6 app.js
  100644 blob 5910ad90fda519a6cc9299d4688679d56dc8d6dd lib.js
  100644 blob 035bf7abf8a572ccf122f71984ed0e9680e8a01d workspace.josh

Update a file, but put it on hold in workspace

  $ cat << EOF > code/app.js
  > async fn main() {
  >   await fetch("https://secret-internal-resource.contoso.com");
  > }
  > EOF

  $ cat << EOF > workspaces/code/workspace.josh
  > :/code:hold[::app.js]
  > EOF

  $ git add .
  $ git commit -q -m "secret update"

Filter and check history

  $ josh-filter ':workspace=workspaces/code'
  $ git log --oneline FILTERED_HEAD
  dcd72a8 secret update
  6620984 first commit

We only see workspace.josh update

  $ git show FILTERED_HEAD
  commit dcd72a8e32bea4c3c86a28e21843e74c3bd351f3
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
  diff --git a/workspace.josh b/workspace.josh
  index 035bf7a..22fbc80 100644
  --- a/workspace.josh
  +++ b/workspace.josh
  @@ -1 +1 @@
  -:/code
  +:/code:hold[::app.js]

We can also exclude workspace.josh itself

  $ josh-filter ':workspace=workspaces/code:exclude[::workspace.josh]'

This makes the commit disappear completely

  $ git log --oneline FILTERED_HEAD
  71f53f9 first commit

Now, let's add another file, but prevent it from appearing

  $ cat << EOF > code/lib2.js
  > fn bar() {}
  > EOF

Also, update app.js and remove hold from it

  $ cat << EOF > code/app.js
  > async fn main() {
  >   const host = process.env.REMOTE_HOST;
  >   await fetch(host);
  > }
  > EOF

  $ cat << EOF > workspaces/code/workspace.josh
  > :/code:hold[::lib2.js]
  > EOF

  $ git add .
  $ git commit -q -m "read env variable"

  $ josh-filter ':workspace=workspaces/code'

Check the resulting history

  $ git log --oneline FILTERED_HEAD
  c44c048 read env variable
  dcd72a8 secret update
  6620984 first commit

Check that files changed in commits are as expected

  $ git show --stat FILTERED_HEAD~1
  commit dcd72a8e32bea4c3c86a28e21843e74c3bd351f3
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
   workspace.josh | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)

  $ git show --stat FILTERED_HEAD
  commit c44c048e8ae5d25b41af2981b87a6b61749fec6a
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      read env variable
  
   app.js         | 3 ++-
   workspace.josh | 2 +-
   2 files changed, 3 insertions(+), 2 deletions(-)

We can also verify that the "offending" version was skipped in filtered history

  $ git show FILTERED_HEAD -- app.js
  commit c44c048e8ae5d25b41af2981b87a6b61749fec6a
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      read env variable
  
  diff --git a/app.js b/app.js
  index 0747fcb..990514f 100644
  --- a/app.js
  +++ b/app.js
  @@ -1,3 +1,4 @@
   async fn main() {
  -  await fetch("http://127.0.0.1");
  +  const host = process.env.REMOTE_HOST;
  +  await fetch(host);
   }
