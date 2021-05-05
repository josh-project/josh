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
  $ docker run -d --network="host" -e JOSH_REMOTE=http://127.0.0.1:8001 -v josh-vol:$(pwd)/git_data esrlabs/josh-proxy:latest > josh.out
ANCHOR_END: docker_josh

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
  remote: To http://127.0.0.1:8001/real_repo.git        
  remote:    f65e94b..37184cc  JOSH_PUSH -> master        
  remote: REWRITE(06361eedf6d6f6d7ada6000481a47363b0f0c3de -> 753d62ca1af960a3d071bb3b40722471228abbf6)        
  remote: 
  remote: 
  updating local tracking ref 'refs/remotes/origin/master'
  
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
ANCHOR_END: library_sync


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
# cleanup
  $ cd ${TESTTMP}
  $ docker stop $(cat josh.out) >/dev/null
  $ kill -9 $(cat ./server_pid)
