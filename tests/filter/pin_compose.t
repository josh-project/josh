  $ export GIT_TREE_FMT='%(objectmode) %(objecttype) %(objectname) %(path)'

  $ export TESTTMP=${PWD}
  $ cd ${TESTTMP}

  $ git init -q repo
  $ cd repo
  $ mkdir -p josh/overlay
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

Also create a workspace with the tree overlay filter

We first select files in josh/overlay, whatever is in there
will take priority over the next tree in the composition filter

  $ mkdir -p workspaces/overlay
  $ cat << EOF > workspaces/overlay/workspace.josh
  > :[
  >   :/code
  >   :/josh/overlay
  > ]
  > EOF

Here's the repo layout at this point:

  $ tree .
  .
  |-- code
  |   |-- app.js
  |   `-- lib.js
  |-- josh
  |   `-- overlay
  `-- workspaces
      `-- overlay
          `-- workspace.josh
  
  6 directories, 3 files

Commit this:

  $ git add .
  $ git commit -q -m "first commit"

Now, filter the ws and check the result

  $ josh-filter ':workspace=workspaces/overlay'
  $ git ls-tree --format="${GIT_TREE_FMT}" -r FILTERED_HEAD
  100644 blob 0747fcb9cd688a7876932dcc30006e6ffa9106d6 app.js
  100644 blob 5910ad90fda519a6cc9299d4688679d56dc8d6dd lib.js
  100644 blob 39dc0f50ad353a5ee880b4a87ecc06dee7b48c92 workspace.josh

Save the OID of app.js before making changes:

  $ export ORIGINAL_APP_OID=$(git ls-tree --format="%(objectname)" FILTERED_HEAD app.js)
  $ echo "${ORIGINAL_APP_OID}"
  0747fcb9cd688a7876932dcc30006e6ffa9106d6

Make next commit: both files will change

  $ cat << EOF > code/app.js
  > async fn main() {
  >   await fetch("http://internal-secret-portal.company.com");
  > }
  > EOF

  $ cat << EOF > code/lib.js
  > fn log() {
  >   console.log("INFO: logged!");
  > }
  > EOF

  $ git add code/app.js code/lib.js

Insert the old app.js OID into the overlay.
Note that we aren't copying the file -- we are directly referencing the OID.
This ensures it's the same entry in git ODB.

  $ git update-index --add --cacheinfo 100644,"${ORIGINAL_APP_OID}","josh/overlay/app.js"
  $ git commit -q -m "second commit"

Verify commit tree looks right:

  $ git ls-tree -r --format="${GIT_TREE_FMT}" HEAD
  100644 blob 1540d15e1bdc499e31ea05703a0daaf520774a85 code/app.js
  100644 blob 627cdb2ef7a3eb1a2b4537ce17fea1d93bfecdd2 code/lib.js
  100644 blob 0747fcb9cd688a7876932dcc30006e6ffa9106d6 josh/overlay/app.js
  100644 blob 39dc0f50ad353a5ee880b4a87ecc06dee7b48c92 workspaces/overlay/workspace.josh

Filter the workspace and check the result:

  $ josh-filter ':workspace=workspaces/overlay'

We can see now that the app.js file was held at the previous version:

  $ git ls-tree --format="${GIT_TREE_FMT}" -r FILTERED_HEAD
  100644 blob 0747fcb9cd688a7876932dcc30006e6ffa9106d6 app.js
  100644 blob 627cdb2ef7a3eb1a2b4537ce17fea1d93bfecdd2 lib.js
  100644 blob 39dc0f50ad353a5ee880b4a87ecc06dee7b48c92 workspace.josh

  $ git show FILTERED_HEAD:app.js
  async fn main() {
    await fetch("http://127.0.0.1");
  }
