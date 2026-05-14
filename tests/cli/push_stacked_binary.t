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

Make stacked changes with binary files

  $ printf '\x00\x01\x02' > binfile
  $ git add binfile
  $ git commit -q -m "Change-Id: bin1"
  $ printf '\x00\x03\x04' > binfile2
  $ git add binfile2
  $ git commit -q -m "Change-Id: bin2"

Set up git config for author

  $ git config user.email "josh@example.com"
  $ git config user.name "Josh Test"

Push with stacked changes containing binary files

  $ josh publish
  To file://${TESTTMP}/remote
   * [new branch]      61d809929bf6b7a29d194f43368936fc82b033db -> @changes/master/josh@example.com/bin1
  
  Pushed 61d809929bf6b7a29d194f43368936fc82b033db to origin/refs/heads/@changes/master/josh@example.com/bin1
  To file://${TESTTMP}/remote
   * [new branch]      115b269a011d493259a125fa941fd790b903175f -> @base/master/josh@example.com/bin1
  
  Pushed 115b269a011d493259a125fa941fd790b903175f to origin/refs/heads/@base/master/josh@example.com/bin1
  To file://${TESTTMP}/remote
   * [new branch]      a4f8ccd2c1cf12aaf3b46eae73e38c71185867e7 -> @changes/master/josh@example.com/bin2
  
  Pushed a4f8ccd2c1cf12aaf3b46eae73e38c71185867e7 to origin/refs/heads/@changes/master/josh@example.com/bin2
  To file://${TESTTMP}/remote
   * [new branch]      115b269a011d493259a125fa941fd790b903175f -> @base/master/josh@example.com/bin2
  
  Pushed 115b269a011d493259a125fa941fd790b903175f to origin/refs/heads/@base/master/josh@example.com/bin2
  To file://${TESTTMP}/remote
   * [new branch]      * -> @heads/master/josh@example.com (glob)
  
  Pushed * to origin/refs/heads/@heads/master/josh@example.com (glob)


Verify binary content is preserved on the change branches

  $ cd ${TESTTMP}/remote
  $ git cat-file -p refs/heads/@changes/master/josh@example.com/bin1:sub1/binfile | xxd
  00000000: 0001 02                                  ...
  $ git cat-file -p refs/heads/@changes/master/josh@example.com/bin2:sub1/binfile2 | xxd
  00000000: 0003 04                                  ...
