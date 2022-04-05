Setup

  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

Clone an empty repo

  $ git clone -q http://localhost:8001/real_repo.git >/dev/null 2>&1
  $ cd real_repo

Commit a file in a root folder

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "add file1"
  [master (root-commit) 0b4cf6c] add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1

Commit a file in a subfolder and push

  $ mkdir sub
  $ echo contents2 > sub/file2
  $ git add sub
  $ git commit -m "add file2" 1> /dev/null
  $ git push 1> /dev/null
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

Check commit SHA1
  $ SHA1=$(git log --max-count=1 --format="%H")
  $ echo "${SHA1}"
  37c3f9a18f21fe53e0be9ea657220ba4537dbca7

Clone subfolder as a workspace

  $ cd ${TESTTMP}
  $ git clone -q http://localhost:8002/real_repo.git:/sub.git
  $ cd sub

Check workspace contents

  $ ls
  file2

Create a new branch and push it

  $ git switch -c new-branch
  git: 'switch' is not a git command. See 'git --help'.
  [1]
  $ git push origin new-branch -o base=refs/heads/master 1> /dev/null
  error: src refspec new-branch does not match any.
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:/sub.git'
  [1]
Check the branch pushed
  $ cd ${TESTTMP}/real_repo
  $ git fetch
  $ [ "${SHA1}" = "$(git log --max-count=1 --format='%H' origin/new-branch)" ] || echo "SHA1 differs after push!"
  fatal: ambiguous argument 'origin/new-branch': unknown revision or path not in the working tree.
  Use '--' to separate paths from revisions, like this:
  'git <command> [<revision>...] -- [<file>...]'
  SHA1 differs after push!

Add one more commit in the workspace and push using implicit prefix in base

  $ cd ${TESTTMP}/sub
  $ echo test > test.txt
  $ git add test.txt
  $ git commit -m "test commit"
  [master 751ef45] test commit
   1 file changed, 1 insertion(+)
   create mode 100644 test.txt
  $ git push origin new-branch -o base=master
  error: src refspec new-branch does not match any.
  error: failed to push some refs to 'http://localhost:8002/real_repo.git:/sub.git'
  [1]

Check the branch again

  $ cd ${TESTTMP}/real_repo
  $ git fetch
  $ [ "${SHA1}" = "$(git log --max-count=1 --skip=1 --format='%H' origin/new-branch)" ] || echo "SHA1 differs after push!"
  fatal: ambiguous argument 'origin/new-branch': unknown revision or path not in the working tree.
  Use '--' to separate paths from revisions, like this:
  'git <command> [<revision>...] -- [<file>...]'
  SHA1 differs after push!

  $ bash ${TESTDIR}/destroy_test_env.sh
  "real_repo.git" = [':/sub']
  refs
  |-- heads
  |-- josh
  |   |-- filtered
  |   |   `-- real_repo.git
  |   |       `-- %3A%2Fsub
  |   |           `-- HEAD
  |   `-- upstream
  |       `-- real_repo.git
  |           |-- HEAD
  |           `-- refs
  |               `-- heads
  |                   `-- master
  |-- namespaces
  `-- tags
  
  11 directories, 3 files
