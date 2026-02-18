Setup

  $ export TESTTMP=${PWD}

Create a test repository with some content

  $ mkdir remote
  $ cd remote
  $ git init -q --bare
  $ cd ..

  $ mkdir local
  $ cd local
  $ git init -q
  $ mkdir -p sub1
  $ echo "file1 content" > sub1/file1
  $ echo "before" > file7
  $ git add .
  $ git commit -q -m "add file1"
  $ git remote add origin ${TESTTMP}/remote
  $ git push -q origin master
  $ cd ..

Clone with josh filter

  $ josh clone ${TESTTMP}/remote :/sub1 filtered
  Added remote 'origin' with filter ':/sub1'
  From file://${TESTTMP}/remote
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://${TESTTMP}/filtered
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: ${TESTTMP}/filtered
  $ cd filtered
  $ tree
  .
  `-- file1
  
  1 directory, 1 file

Make multiple changes with Change-Ids for split testing

  $ echo "contents2" > file2
  $ git add file2
  $ git commit -q -m "Change-Id: 1234"
  $ echo "contents2" > file7
  $ git add file7
  $ git commit -q -m "Change: foo7"
  $ echo "contents3" > file2
  $ git add file2
  $ git commit -q -m "Change-Id: 1235"
  $ git log --decorate --graph --pretty="%s %d"
  * Change-Id: 1235  (HEAD -> master)
  * Change: foo7 
  * Change-Id: 1234 
  * add file1  (origin/master, origin/HEAD)

Set up git config for author

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Push with split mode (should create multiple refs for each change)

  $ josh push --split
  To file://${TESTTMP}/remote
   * [new branch]      c61c37f4a3d5eb447f41dde15620eee1a181d60b -> @changes/master/josh@example.com/1234
  
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@changes/master/josh@example.com/1234
  To file://${TESTTMP}/remote
   * [new branch]      6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d -> @base/master/josh@example.com/1234
  
  Pushed 6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d to origin/refs/heads/@base/master/josh@example.com/1234
  To file://${TESTTMP}/remote
   * [new branch]      ba95dae3e5cf8fb0db28a931081e3a28f61fc94b -> @changes/master/josh@example.com/foo7
  
  Pushed ba95dae3e5cf8fb0db28a931081e3a28f61fc94b to origin/refs/heads/@changes/master/josh@example.com/foo7
  To file://${TESTTMP}/remote
   * [new branch]      6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d -> @base/master/josh@example.com/foo7
  
  Pushed 6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d to origin/refs/heads/@base/master/josh@example.com/foo7
  To file://${TESTTMP}/remote
   * [new branch]      ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 -> @changes/master/josh@example.com/1235
  
  Pushed ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 to origin/refs/heads/@changes/master/josh@example.com/1235
  To file://${TESTTMP}/remote
   * [new branch]      c61c37f4a3d5eb447f41dde15620eee1a181d60b -> @base/master/josh@example.com/1235
  
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@base/master/josh@example.com/1235
  To file://${TESTTMP}/remote
   * [new branch]      e8a69ac0518e72aec932b6f66f17670130cd1d0f -> @heads/master/josh@example.com
  
  Pushed e8a69ac0518e72aec932b6f66f17670130cd1d0f to origin/refs/heads/@heads/master/josh@example.com

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote . | grep "@" | sort
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/heads/@base/master/josh@example.com/1234 (esc)
  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d\trefs/heads/@base/master/josh@example.com/foo7 (esc)
  ba95dae3e5cf8fb0db28a931081e3a28f61fc94b\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@base/master/josh@example.com/1235 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  e8a69ac0518e72aec932b6f66f17670130cd1d0f\trefs/heads/@heads/master/josh@example.com (esc)
  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9\trefs/heads/@changes/master/josh@example.com/1235 (esc)

  $ git log --all --decorate --graph --pretty="%s %d %H"
  * Change-Id: 1235  (@changes/master/josh@example.com/1235) ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9
  | *   (@changes/master/josh@example.com/foo7) ba95dae3e5cf8fb0db28a931081e3a28f61fc94b
  | | * Change-Id: 1235  (@heads/master/josh@example.com) e8a69ac0518e72aec932b6f66f17670130cd1d0f
  | | *   115911beff2c43af69fb8b00efc50b6057b4174d
  | |/  
  |/|   
  * | Change-Id: 1234  (@changes/master/josh@example.com/1234, @base/master/josh@example.com/1235) c61c37f4a3d5eb447f41dde15620eee1a181d60b
  |/  
  * add file1  (HEAD -> master, @base/master/josh@example.com/foo7, @base/master/josh@example.com/1234) 6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d

Test that we can fetch the split refs back

  $ cd ${TESTTMP}/filtered
  $ josh fetch
  From file://${TESTTMP}/remote
   * [new branch]      @base/master/josh@example.com/1234 -> refs/josh/remotes/origin/@base/master/josh@example.com/1234
   * [new branch]      @base/master/josh@example.com/1235 -> refs/josh/remotes/origin/@base/master/josh@example.com/1235
   * [new branch]      @base/master/josh@example.com/foo7 -> refs/josh/remotes/origin/@base/master/josh@example.com/foo7
   * [new branch]      @changes/master/josh@example.com/1234 -> refs/josh/remotes/origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> refs/josh/remotes/origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> refs/josh/remotes/origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> refs/josh/remotes/origin/@heads/master/josh@example.com
  
  From file://${TESTTMP}/filtered
   * [new branch]      @base/master/josh@example.com/1234 -> origin/@base/master/josh@example.com/1234
   * [new branch]      @base/master/josh@example.com/1235 -> origin/@base/master/josh@example.com/1235
   * [new branch]      @base/master/josh@example.com/foo7 -> origin/@base/master/josh@example.com/foo7
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
  
  Fetched from remote: origin

  $ git log --all --decorate --graph --pretty="%s %d %H"
  * Change-Id: 1235  (HEAD -> master) 8fee494b5170edb463fc623d03d562118cebe88e
  * Change: foo7  cadc8f164b24465285d8ec413e0325a6341e4453
  | * Change-Id: 1235  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9
  | | *   ba95dae3e5cf8fb0db28a931081e3a28f61fc94b
  | | | * Change-Id: 1235  e8a69ac0518e72aec932b6f66f17670130cd1d0f
  | | | *   115911beff2c43af69fb8b00efc50b6057b4174d
  | | |/  
  | |/|   
  | * | Change-Id: 1234  c61c37f4a3d5eb447f41dde15620eee1a181d60b
  | |/  
  | * add file1  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d
  | * Change-Id: 1235  (origin/@changes/master/josh@example.com/1235) 96da92a9021ee186e1e9dd82305ddebfd1153ed5
  |/  
  | *   (origin/@changes/master/josh@example.com/foo7) efa55fb3fda9dee1c5c1cb135827f2900a9fecbe
  | | * Change-Id: 1235  (origin/@heads/master/josh@example.com) d82c0a1a74d2dab593c5c8ad02055e5994d2aff9
  | | *   51310e834ba7c7f2f034352a39a308bd86e5dd70
  | |/  
  |/|   
  * | Change-Id: 1234  (origin/@changes/master/josh@example.com/1234, origin/@base/master/josh@example.com/1235) 43d6fcc9e7a81452d7343c78c0102f76027717fb
  |/  
  * add file1  (origin/master, origin/HEAD, origin/@base/master/josh@example.com/foo7, origin/@base/master/josh@example.com/1234) 5f2928c89c4dcc7f5a8c59ef65734a83620cefee
  * cache  3c2c2237ae79b148f5a4ca12279f75ab6029fe2b

Test normal push still works

  $ echo "contents4" > file2
  $ git add file2
  $ git commit -q -m "add file4" -m "Change-Id: 1236"
  $ josh push
  To file://${TESTTMP}/remote
     6ed6c1c..46af19d  46af19d75e628e41acb704f2fcae3973ed780d4a -> master
  
  Pushed 46af19d75e628e41acb704f2fcae3973ed780d4a to origin/master

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents4
