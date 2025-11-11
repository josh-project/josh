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
  Added remote 'origin' with filter ':/sub1:prune=trivial-merge'
  From $TESTTMP/remote
   * [new branch]      master     -> refs/josh/remotes/origin/master
  
  From file://$TESTTMP/filtered
   * [new branch]      master     -> origin/master
  
  Fetched from remote: origin
  Already on 'master'
  
  Cloned repository to: filtered
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
  To $TESTTMP/remote
   * [new branch]      c61c37f4a3d5eb447f41dde15620eee1a181d60b -> @changes/master/josh@example.com/1234
  
  Pushed c61c37f4a3d5eb447f41dde15620eee1a181d60b to origin/refs/heads/@changes/master/josh@example.com/1234
  To $TESTTMP/remote
   * [new branch]      9da166dcfa8650e04c7e39c54a61b7fa0b69ef4f -> @changes/master/josh@example.com/foo7
  
  Pushed 9da166dcfa8650e04c7e39c54a61b7fa0b69ef4f to origin/refs/heads/@changes/master/josh@example.com/foo7
  To $TESTTMP/remote
   * [new branch]      ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 -> @changes/master/josh@example.com/1235
  
  Pushed ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9 to origin/refs/heads/@changes/master/josh@example.com/1235
  To $TESTTMP/remote
   * [new branch]      52310103e5d9a8e55df1a7766789a1af04d8601b -> @heads/master/josh@example.com
  
  Pushed 52310103e5d9a8e55df1a7766789a1af04d8601b to origin/refs/heads/@heads/master/josh@example.com

Verify the refs were created in the remote

  $ cd ${TESTTMP}/remote
  $ git ls-remote . | grep "@" | sort
  52310103e5d9a8e55df1a7766789a1af04d8601b\trefs/heads/@heads/master/josh@example.com (esc)
  9da166dcfa8650e04c7e39c54a61b7fa0b69ef4f\trefs/heads/@changes/master/josh@example.com/foo7 (esc)
  c61c37f4a3d5eb447f41dde15620eee1a181d60b\trefs/heads/@changes/master/josh@example.com/1234 (esc)
  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9\trefs/heads/@changes/master/josh@example.com/1235 (esc)

  $ git log --all --decorate --graph --pretty="%s %d %H"
  * Change-Id: 1235  (@changes/master/josh@example.com/1235) ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9
  | *   (@changes/master/josh@example.com/foo7) 9da166dcfa8650e04c7e39c54a61b7fa0b69ef4f
  | | * Change-Id: 1235  (@heads/master/josh@example.com) 52310103e5d9a8e55df1a7766789a1af04d8601b
  | | *   48f307ad20210547fdf339d0b0d7ee02bc702c3d
  | |/  
  |/|   
  * | Change-Id: 1234  (@changes/master/josh@example.com/1234) c61c37f4a3d5eb447f41dde15620eee1a181d60b
  |/  
  * add file1  (HEAD -> master) 6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d

Test that we can fetch the split refs back

  $ cd ${TESTTMP}/filtered
  $ josh fetch
  From $TESTTMP/remote
   * [new branch]      @changes/master/josh@example.com/1234 -> refs/josh/remotes/origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> refs/josh/remotes/origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> refs/josh/remotes/origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> refs/josh/remotes/origin/@heads/master/josh@example.com
  
  From file://$TESTTMP/filtered
   * [new branch]      @changes/master/josh@example.com/1234 -> origin/@changes/master/josh@example.com/1234
   * [new branch]      @changes/master/josh@example.com/1235 -> origin/@changes/master/josh@example.com/1235
   * [new branch]      @changes/master/josh@example.com/foo7 -> origin/@changes/master/josh@example.com/foo7
   * [new branch]      @heads/master/josh@example.com -> origin/@heads/master/josh@example.com
  
  Fetched from remote: origin

  $ git log --all --decorate --graph --pretty="%s %d %H"
  * Change-Id: 1235  (HEAD -> master) 8fee494b5170edb463fc623d03d562118cebe88e
  * Change: foo7  cadc8f164b24465285d8ec413e0325a6341e4453
  | * Change-Id: 1235  ef7c3c85ad4c5875f308003d42a6e11d9b14aeb9
  | | *   9da166dcfa8650e04c7e39c54a61b7fa0b69ef4f
  | | | * Change-Id: 1235  52310103e5d9a8e55df1a7766789a1af04d8601b
  | | | *   48f307ad20210547fdf339d0b0d7ee02bc702c3d
  | | |/  
  | |/|   
  | * | Change-Id: 1234  c61c37f4a3d5eb447f41dde15620eee1a181d60b
  | |/  
  | * add file1  6ed6c1ca90cb15fe4edf8d133f0e2e44562aa77d
  | * Change-Id: 1235  (origin/@changes/master/josh@example.com/1235) 96da92a9021ee186e1e9dd82305ddebfd1153ed5
  |/  
  | *   (origin/@changes/master/josh@example.com/foo7) 7a54645bd1415d8b911ea129f90fb962799846d2
  | | * Change-Id: 1235  (origin/@heads/master/josh@example.com) 7fc3588b28a7c0e18be92f3e8303ccf632072804
  | | *   bc99e4e2bb4f77e86e65630057da2cea96110852
  | |/  
  |/|   
  * | Change-Id: 1234  (origin/@changes/master/josh@example.com/1234) 43d6fcc9e7a81452d7343c78c0102f76027717fb
  |/  
  * add file1  (origin/master, origin/HEAD) 5f2928c89c4dcc7f5a8c59ef65734a83620cefee
  * Notes added by 'git_note_create' from libgit2  725a17751b9dc03b1696fb894d0643c5b6f0397d
  * Notes added by 'git_note_create' from libgit2  030ef005644909d7f6320dcd99684a36860fb7d9

Test normal push still works

  $ echo "contents4" > file2
  $ git add file2
  $ git commit -q -m "add file4" -m "Change-Id: 1236"
  $ josh push
  To $TESTTMP/remote
     6ed6c1c..46af19d  46af19d75e628e41acb704f2fcae3973ed780d4a -> master
  
  Pushed 46af19d75e628e41acb704f2fcae3973ed780d4a to origin/master

Verify normal push worked

  $ cd ${TESTTMP}/local
  $ git pull -q --rebase origin master
  $ cat sub1/file2
  contents4
