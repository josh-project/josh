  $ export TESTTMP=${PWD}
  $ export GIT_AUTHOR_NAME=Josh
  $ export GIT_AUTHOR_EMAIL=josh@example.com
  $ export GIT_AUTHOR_DATE="2005-04-07T22:13:13"
  $ export GIT_COMMITTER_NAME=Josh
  $ export GIT_COMMITTER_EMAIL=josh@example.com
  $ export GIT_COMMITTER_DATE="2005-04-07T22:13:13"

# setting up the git server
ANCHOR: git_setup
  $ git init --bare ./remote/real_repo.git/
  Initialized empty Git repository in */real_repo.git/ (glob)
  $ git config -f ./remote/real_repo.git/config http.receivepack true
ANCHOR_END: git_setup

ANCHOR: git_server
  $ GIT_DIR=./remote/ GIT_PROJECT_ROOT=${TESTTMP}/remote/ GIT_HTTP_EXPORT_ALL=1 hyper-cgi-test-server\
  >  --port=8001\
  >  --dir=./remote/\
  >  --cmd=git\
  >  --args=http-backend\
  >  > ./hyper-cgi-test-server.out 2>&1 &
  $ echo $! > ./server_pid
ANCHOR_END: git_server

# waiting for the git server to be running
  $ until curl -s http://localhost:8001/
  > do
  >     sleep 0.1
  > done

ANCHOR: clone
  $ git clone http://localhost:8001/real_repo.git
  Cloning into 'real_repo'...
  warning: You appear to have cloned an empty repository.
ANCHOR_END: clone

ANCHOR: populate
  $ cd real_repo
  $ sh ${TESTDIR}/populate.sh > ../populate.out

  $ git push origin HEAD
  To http://localhost:8001/real_repo.git
   * [new branch]      HEAD -> master

  $ tree
  .
  |-- application1
  |   `-- app.c
  |-- application2
  |   `-- guide.c
  |-- doc
  |   |-- guide.md
  |   |-- library1.md
  |   `-- library2.md
  |-- library1
  |   `-- lib1.h
  `-- library2
      `-- lib2.h

  5 directories, 7 files
  $ git log --oneline --graph
  * f65e94b Add documentation
  * f240612 Add application2
  * 0a7f473 Add library2
  * 1079ef1 Add application1
  * 6476861 Add library1
ANCHOR_END: populate

  $ cd ${TESTTMP}

# cloning a workspace
ANCHOR: clone_workspace
  $ josh clone http://localhost:8001/real_repo.git :workspace=application1 ./application1
  Added remote 'origin' with filter ':workspace=application1'
  Cloned repository to: */application1 (glob)
  $ cd application1
  $ tree
  .
  `-- app.c

  0 directories, 1 file
  $ git log -2
  commit * (glob)
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000

      Add application1
ANCHOR_END: clone_workspace

ANCHOR: library_ws
  $ echo "modules/lib1 = :/library1" >> workspace.josh

  $ git add workspace.josh

  $ git commit -m "Map library1 to the application1 workspace"
  [master *] Map library1 to the application1 workspace (glob)
   1 file changed, 1 insertion(+)
   create mode 100644 workspace.josh
ANCHOR_END: library_ws

ANCHOR: library_sync
  $ josh push
  Pushed * to origin/master (glob)
  $ josh pull
  Pulled from remote: origin
ANCHOR_END: library_sync

ANCHOR: library_sync2
  $ tree
  .
  |-- app.c
  |-- modules
  |   `-- lib1
  |       `-- lib1.h
  `-- workspace.josh

  2 directories, 3 files
  $ git log --graph --oneline
  *   * Map library1 to the application1 workspace (glob)
  |\
  | * * Add library1 (glob)
  * * Add application1 (glob)
ANCHOR_END: library_sync2

ANCHOR: real_repo
  $ cd ../real_repo
  $ git pull origin master
  From http://localhost:8001/real_repo
   * branch            master     -> FETCH_HEAD
     f65e94b..* master     -> origin/master (glob)
  Updating f65e94b..* (glob)
  Fast-forward
   application1/workspace.josh | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 application1/workspace.josh
  Current branch master is up to date.

  $ tree
  .
  |-- application1
  |   |-- app.c
  |   `-- workspace.josh
  |-- application2
  |   `-- guide.c
  |-- doc
  |   |-- guide.md
  |   |-- library1.md
  |   `-- library2.md
  |-- library1
  |   `-- lib1.h
  `-- library2
      `-- lib2.h

  5 directories, 8 files
  $ git log --graph --oneline
  * * Map library1 to the application1 workspace (glob)
  * f65e94b Add documentation
  * f240612 Add application2
  * 0a7f473 Add library2
  * 1079ef1 Add application1
  * 6476861 Add library1

ANCHOR_END: real_repo

  $ cd ${TESTTMP}

ANCHOR: application2
  $ josh clone http://localhost:8001/real_repo.git :workspace=application2 ./application2
  Added remote 'origin' with filter ':workspace=application2'
  Cloned repository to: */application2 (glob)
  $ cd application2
  $ echo "libs/lib1 = :/library1" >> workspace.josh
  $ echo "libs/lib2 = :/library2" >> workspace.josh
  $ git add workspace.josh && git commit -m "Create workspace for application2"
  [master *] Create workspace for application2 (glob)
   1 file changed, 2 insertions(+)
   create mode 100644 workspace.josh
ANCHOR_END: application2

ANCHOR: app2_sync
  $ josh push
  Pushed * to origin/master (glob)
  $ josh pull
  Pulled from remote: origin
ANCHOR_END: app2_sync

ANCHOR: app2_files
  $ tree
  .
  |-- guide.c
  |-- libs
  |   |-- lib1
  |   |   `-- lib1.h
  |   `-- lib2
  |       `-- lib2.h
  `-- workspace.josh

  3 directories, 4 files
ANCHOR_END: app2_files

ANCHOR: app2_hist
  $ git log --oneline --graph
  *   * Create workspace for application2 (glob)
  |\
  | * * Add library2 (glob)
  | * * Add library1 (glob)
  * * Add application2 (glob)
ANCHOR_END: app2_hist

ANCHOR: fix_typo
  $ sed -i 's/41/42/' libs/lib1/lib1.h
  $ git commit -a -m "fix lib1 typo"
  [master *] fix lib1 typo (glob)
   1 file changed, 1 insertion(+), 1 deletion(-)
ANCHOR_END: fix_typo

ANCHOR: push_change
  $ josh push
  Pushed * to origin/master (glob)
ANCHOR_END: push_change

ANCHOR: app1_pull
  $ cd ../application1
  $ josh pull
  Pulled from remote: origin
ANCHOR_END: app1_pull

ANCHOR: app1_log
  $ git log --oneline --graph
  * * fix lib1 typo (glob)
  *   * Map library1 to the application1 workspace (glob)
  |\
  | * * Add library1 (glob)
  * * Add application1 (glob)
ANCHOR_END: app1_log

# cleanup
  $ cd ${TESTTMP}
  $ kill -9 $(cat ./server_pid)
