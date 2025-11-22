  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents1 > sub1/file2
  $ chmod +x sub1/file2
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null
  $ git ls-tree -r HEAD
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tsub1/file1 (esc)
  100755 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tsub1/file2 (esc)

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir st
  $ cat > st/config.josh <<EOF
  > :/sub1::file1
  > :/sub1::file2
  > ::sub2/subsub/
  > EOF
  $ git add st
  $ git commit -m "add st" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/josh/master
  4151198c71b2a1ecc2ff632ecf335c3f1604926b
  [2] :+st/config
  [2] :[
      :/sub1:[
          ::file1
          ::file2
      ]
      ::sub2/subsub/
  ]
  [3] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * add st
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ git ls-tree HEAD
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile1 (esc)
  100755 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile2 (esc)
  040000 tree 39ba55859ffd8fca4931d1426510f486b3285e07\tst (esc)
  040000 tree 81b2a24c53f9090c6f6a23176a2a5660e6f48317\tsub2 (esc)
  $ tree
  .
  |-- file1
  |-- file2
  |-- st
  |   `-- config.josh
  `-- sub2
      `-- subsub
          `-- file2
  
  4 directories, 4 files

