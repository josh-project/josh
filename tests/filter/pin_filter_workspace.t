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

Create a workspace: the :pin filter must be applicable per-commit, so it should
be tied to commit sha1 either via workspace or via hook. Otherwise we are going
to pin a file in every commit, resulting in no versions of the file appearing at all.
Don't pin anything yet.

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

Update a file, but put it on pin in workspace

  $ cat << EOF > code/app.js
  > async fn main() {
  >   await fetch("https://secret-internal-resource.contoso.com");
  > }
  > EOF

  $ cat << EOF > workspaces/code/workspace.josh
  > :/code:pin[::app.js]
  > EOF

  $ git add .
  $ git commit -q -m "secret update"

Filter and check history

  $ josh-filter ':workspace=workspaces/code'
  $ git log --oneline FILTERED_HEAD
  4f83a36 secret update
  6620984 first commit

We only see workspace.josh update

  $ git show FILTERED_HEAD
  commit 4f83a362554fde79389596222637db9084e028bc
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
  diff --git a/workspace.josh b/workspace.josh
  index 035bf7a..801a6f7 100644
  --- a/workspace.josh
  +++ b/workspace.josh
  @@ -1 +1 @@
  -:/code
  +:/code:pin[::app.js]

We can also exclude workspace.josh itself

  $ josh-filter ':workspace=workspaces/code:exclude[::workspace.josh]'

This makes the commit disappear completely

  $ git log --oneline FILTERED_HEAD
  71f53f9 first commit

Now, let's add another file, but prevent it from appearing

  $ cat << EOF > code/lib2.js
  > fn bar() {}
  > EOF

Also, update app.js and remove pin from it

  $ cat << EOF > code/app.js
  > async fn main() {
  >   const host = process.env.REMOTE_HOST;
  >   await fetch(host);
  > }
  > EOF

  $ cat << EOF > workspaces/code/workspace.josh
  > :/code:pin[::lib2.js]
  > EOF

  $ git add .
  $ git commit -q -m "read env variable"

  $ josh-filter ':workspace=workspaces/code'

Check the resulting history

  $ git log --oneline FILTERED_HEAD
  824fe83 read env variable
  4f83a36 secret update
  6620984 first commit

Check that files changed in commits are as expected

  $ git show --stat FILTERED_HEAD~1
  commit 4f83a362554fde79389596222637db9084e028bc
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
   workspace.josh | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)

  $ git show --stat FILTERED_HEAD
  commit 824fe83d4a74a71fd7bec25756166863e063b932
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      read env variable
  
   app.js         | 3 ++-
   workspace.josh | 2 +-
   2 files changed, 3 insertions(+), 2 deletions(-)

We can also verify that the "offending" version was skipped in filtered history

  $ git show FILTERED_HEAD -- app.js
  commit 824fe83d4a74a71fd7bec25756166863e063b932
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
