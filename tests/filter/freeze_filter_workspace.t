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

Create a workspace: the :freeze filter must be applicable per-commit, so it should
be tied to commit sha1 either via workspace or via hook. Otherwise we are going
to prevent the file appearing in every commit, resulting in no versions of the
file making it to the filtered history at all. Don't freeze anything yet.

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

Update a file, but freeze it in workspace

  $ cat << EOF > code/app.js
  > async fn main() {
  >   await fetch("https://secret-internal-resource.contoso.com");
  > }
  > EOF

  $ cat << EOF > workspaces/code/workspace.josh
  > :/code:freeze[::app.js]
  > EOF

  $ git add .
  $ git commit -q -m "secret update"

Filter and check history

  $ josh-filter ':workspace=workspaces/code'
  $ git log --oneline FILTERED_HEAD
  7a6caa2 secret update
  6620984 first commit

We only see workspace.josh update

  $ git show FILTERED_HEAD
  commit 7a6caa204512b03eb13a3b7890248cd783870f31
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
  diff --git a/workspace.josh b/workspace.josh
  index 035bf7a..d44f76c 100644
  --- a/workspace.josh
  +++ b/workspace.josh
  @@ -1 +1 @@
  -:/code
  +:/code:freeze[::app.js]

We can also exclude workspace.josh itself

  $ josh-filter ':workspace=workspaces/code:exclude[::workspace.josh]'

This makes the commit disappear completely

  $ git log --oneline FILTERED_HEAD
  71f53f9 first commit

Now, let's add another file, but prevent it from appearing

  $ cat << EOF > code/lib2.js
  > fn bar() {}
  > EOF

Also, update app.js and unfreeze it

  $ cat << EOF > code/app.js
  > async fn main() {
  >   const host = process.env.REMOTE_HOST;
  >   await fetch(host);
  > }
  > EOF

  $ cat << EOF > workspaces/code/workspace.josh
  > :/code:freeze[::lib2.js]
  > EOF

  $ git add .
  $ git commit -q -m "read env variable"

  $ josh-filter ':workspace=workspaces/code'

Check the resulting history

  $ git log --oneline FILTERED_HEAD
  413bdd8 read env variable
  7a6caa2 secret update
  6620984 first commit

Check that files changed in commits are as expected

  $ git show --stat FILTERED_HEAD~1
  commit 7a6caa204512b03eb13a3b7890248cd783870f31
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
   workspace.josh | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)

  $ git show --stat FILTERED_HEAD
  commit 413bdd8bc39bcbd4f292d72c687205fe0978f84e
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      read env variable
  
   app.js         | 3 ++-
   workspace.josh | 2 +-
   2 files changed, 3 insertions(+), 2 deletions(-)

We can also verify that the "offending" version was skipped in filtered history

  $ git show FILTERED_HEAD -- app.js
  commit 413bdd8bc39bcbd4f292d72c687205fe0978f84e
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
