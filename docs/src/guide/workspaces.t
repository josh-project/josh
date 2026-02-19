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
  $ mkdir git_data

# starting josh
ANCHOR: docker_josh
$ docker run -d --network="host" -e JOSH_REMOTE=http://127.0.0.1:8001 -v josh-vol:$(pwd)/git_data joshproject/josh-proxy:latest > josh.out
ANCHOR_END: docker_josh

# For simplicity sake, this test actually uses a locally build josh instance 
# rather than the docker hub version.
# Note: have to run cargo install on josh-proxy first

  $ josh-proxy --port=8000 --local=$(pwd)/git_data --remote=http://localhost:8001 > josh.out &
  $ echo $! > ./proxy_pid

# waiting for josh to be running
  $ until curl -s http://localhost:8000/
  > do
  >     sleep 0.1
  > done

# cloning a workspace
ANCHOR: clone_workspace
  $ git clone http://127.0.0.1:8000/real_repo.git:workspace=application1.git application1
  Cloning into 'application1'...
  $ cd application1
  $ tree
  .
  `-- app.c
  
  0 directories, 1 file
  $ git log -2
  commit 50cd6112e173df4cac1aca9cb88b5c2a180bc526
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      Add application1
ANCHOR_END: clone_workspace

ANCHOR: library_ws
  $ echo "modules/lib1 = :/library1" >> workspace.josh

  $ git add workspace.josh

  $ git commit -m "Map library1 to the application1 workspace"
  [master 06361ee] Map library1 to the application1 workspace
   1 file changed, 1 insertion(+)
   create mode 100644 workspace.josh
ANCHOR_END: library_ws

ANCHOR: library_sync
  $ git sync origin HEAD
    HEAD -> refs/heads/master
  From http://127.0.0.1:8000/real_repo.git:workspace=application1
   * branch            753d62ca1af960a3d071bb3b40722471228abbf6 -> FETCH_HEAD
  HEAD is now at 753d62c Map library1 to the application1 workspace
  Pushing to http://127.0.0.1:8000/real_repo.git:workspace=application1.git
  POST git-receive-pack (477 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    f65e94b..37184cc  JOSH_PUSH -> master        
  remote: REWRITE(06361eedf6d6f6d7ada6000481a47363b0f0c3de -> 753d62ca1af960a3d071bb3b40722471228abbf6)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  
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
  *   753d62c Map library1 to the application1 workspace
  |\  
  | * 366adba Add library1
  * 50cd611 Add application1
ANCHOR_END: library_sync2

ANCHOR: real_repo
  $ cd ../real_repo
  $ git pull origin master
  From http://localhost:8001/real_repo
   * branch            master     -> FETCH_HEAD
     f65e94b..37184cc  master     -> origin/master
  Updating f65e94b..37184cc
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
  * 37184cc Map library1 to the application1 workspace
  * f65e94b Add documentation
  * f240612 Add application2
  * 0a7f473 Add library2
  * 1079ef1 Add application1
  * 6476861 Add library1

ANCHOR_END: real_repo

  $ cd ${TESTTMP}

ANCHOR: application2
  $ git clone http://127.0.0.1:8000/real_repo.git:workspace=application2.git application2
  Cloning into 'application2'...
  $ cd application2
  $ echo "libs/lib1 = :/library1" >> workspace.josh
  $ echo "libs/lib2 = :/library2" >> workspace.josh
  $ git add workspace.josh && git commit -m "Create workspace for application2"
  [master 566a489] Create workspace for application2
   1 file changed, 2 insertions(+)
   create mode 100644 workspace.josh
ANCHOR_END: application2

ANCHOR: app2_sync
  $ git sync origin HEAD
    HEAD -> refs/heads/master
  From http://127.0.0.1:8000/real_repo.git:workspace=application2
   * branch            5115fd2a5374cbc799da61a228f7fece3039250b -> FETCH_HEAD
  HEAD is now at 5115fd2 Create workspace for application2
  Pushing to http://127.0.0.1:8000/real_repo.git:workspace=application2.git
  POST git-receive-pack (478 bytes)
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    37184cc..feb3a5b  JOSH_PUSH -> master        
  remote: REWRITE(566a4899f0697d0bde1ba064ed81f0654a316332 -> 5115fd2a5374cbc799da61a228f7fece3039250b)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  
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
  *   5115fd2 Create workspace for application2
  |\  
  | * ffaf58d Add library2
  | * f4e4e40 Add library1
  * ee8a5d7 Add application2
ANCHOR_END: app2_hist

ANCHOR: fix_typo
  $ sed -i 's/41/42/' libs/lib1/lib1.h
  $ git commit -a -m "fix lib1 typo"
  [master 82238bf] fix lib1 typo
   1 file changed, 1 insertion(+), 1 deletion(-)
ANCHOR_END: fix_typo

ANCHOR: push_change
  $ git push origin master
  remote: josh-proxy        
  remote: response from upstream:        
  remote: To http://localhost:8001/real_repo.git        
  remote:    feb3a5b..31e8fab  JOSH_PUSH -> master        
  remote: 
  remote: 
  To http://127.0.0.1:8000/real_repo.git:workspace=application2.git
     5115fd2..82238bf  master -> master
ANCHOR_END: push_change

ANCHOR: app1_pull
  $ cd ../application1
  $ git pull
  From http://127.0.0.1:8000/real_repo.git:workspace=application1
   + 06361ee...c64b765 master     -> origin/master  (forced update)
  Updating 753d62c..c64b765
  Fast-forward
   modules/lib1/lib1.h | 2 +-
   1 file changed, 1 insertion(+), 1 deletion(-)
  Current branch master is up to date.
ANCHOR_END: app1_pull

ANCHOR: app1_log
  $ git log --oneline --graph
  * c64b765 fix lib1 typo
  *   753d62c Map library1 to the application1 workspace
  |\  
  | * 366adba Add library1
  * 50cd611 Add application1
ANCHOR_END: app1_log

# cleanup
  $ cd ${TESTTMP}
$ docker stop $(cat josh.out) >/dev/null
  $ kill -9 $(cat ./server_pid)
  $ kill -9 $(cat ./proxy_pid)
