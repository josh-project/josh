  $ export GIT_TREE_FMT='%(objectmode) %(objecttype) %(objectname) %(path)'

  $ export TESTTMP=${PWD}
  $ cd ${TESTTMP}

Similar scenario to pin_filter_workspace.t, except here it's using
filter hooks as opposed to workspace.josh

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

  $ git add .
  $ git commit -q -m "first commit"

Add note with basic filter - no pin yet

  $ git notes add -m ':/code' -f

Update files, but pin one file using git notes

  $ cat << EOF > code/app.js
  > async fn main() {
  >   await fetch("https://secret-internal-resource.contoso.com");
  > }
  > EOF

  $ cat << EOF > code/lib2.js
  > fn foo() {}
  > EOF

  $ git add .
  $ git commit -q -m "secret update"

Add note with pin filter for this commit

  $ git notes add -m ':/code:pin[::app.js]' -f

Filter using the hook

  $ josh-filter ':hook=commits'
  e22b4d031f61b2d443a11700627fa73011bfd95f

Check the filtered history

  $ git log --oneline FILTERED_HEAD
  e22b4d0 secret update
  71f53f9 first commit

Verify that the secret update commit doesn't show app.js changes

  $ git show FILTERED_HEAD
  commit e22b4d031f61b2d443a11700627fa73011bfd95f
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      secret update
  
  diff --git a/lib2.js b/lib2.js
  new file mode 100644
  index 0000000..8f3b7ef
  --- /dev/null
  +++ b/lib2.js
  @@ -0,0 +1 @@
  +fn foo() {}

Add another file and use hook to hold it

  $ cat << EOF > code/lib3.js
  > fn bar() {}
  > EOF

Also update app.js to remove the secret - remove pin from it

  $ cat << EOF > code/app.js
  > async fn main() {
  >   const host = process.env.REMOTE_HOST;
  >   await fetch(host);
  > }
  > EOF

  $ git add .
  $ git commit -q -m "read env variable"

Add note to pin the new file and allow app.js changes

  $ git notes add -m ':/code:pin[::lib3.js]' -f

  $ josh-filter ':hook=commits'
  6b712b21b99511e8b40334a96ef266d2a38f2e94

Check the resulting history

  $ git log --oneline FILTERED_HEAD
  6b712b2 read env variable
  e22b4d0 secret update
  71f53f9 first commit

Verify that app.js changes are now visible but lib3.js is pinned

  $ git show FILTERED_HEAD -- app.js
  commit 6b712b21b99511e8b40334a96ef266d2a38f2e94
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

  $ git ls-tree --format="${GIT_TREE_FMT}" -r FILTERED_HEAD
  100644 blob 990514fe4034b4d8dac7ffa05d4a74331b57cb21 app.js
  100644 blob 5910ad90fda519a6cc9299d4688679d56dc8d6dd lib.js
  100644 blob 8f3b7ef112a0f4951016967f520b9399c02f902d lib2.js
