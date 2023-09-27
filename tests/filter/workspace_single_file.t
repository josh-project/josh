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

  $ mkdir ws
  $ cat > ws/workspace.josh <<EOF
  > :/sub1::file1
  > :/sub1::file2
  > ::sub2/subsub/
  > EOF
  $ git add ws
  $ git commit -m "add ws" 1> /dev/null

  $ mkdir ws2
  $ cat > ws2/workspace.josh <<EOF
  > :/sub1::file1
  > :/sub1::file2
  > ::sub2/subsub
  > EOF
  $ git add ws2
  $ git commit -m "add ws2" 1> /dev/null

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] ::file1
  [1] ::file2
  [1] :[
      ::file1
      ::file2
  ]
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] :[
      :/sub1:[
          ::file1
          ::file2
      ]
      ::sub2/subsub/
  ]
  [2] :workspace=ws

  $ git log --graph --pretty=%s refs/josh/master
  * add ws
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ git ls-tree HEAD
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile1 (esc)
  100755 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile2 (esc)
  040000 tree 81b2a24c53f9090c6f6a23176a2a5660e6f48317\tsub2 (esc)
  100644 blob 63f07c908400fab3a663e52e480970d8458bc86a\tworkspace.josh (esc)
  $ tree
  .
  |-- file1
  |-- file2
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 4 files

  $ josh-filter -s :workspace=ws2 master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] ::file1
  [1] ::file2
  [1] :[
      ::file1
      ::file2
  ]
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] ::sub2/subsub
  [2] :[
      :/sub1:[
          ::file1
          ::file2
      ]
      ::sub2/subsub
  ]
  [2] :[
      :/sub1:[
          ::file1
          ::file2
      ]
      ::sub2/subsub/
  ]
  [2] :workspace=ws
  [2] :workspace=ws2

  $ git log --graph --pretty=%s refs/josh/master
  * add ws2
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ git ls-tree HEAD
  100644 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile1 (esc)
  100755 blob a024003ee1acc6bf70318a46e7b6df651b9dc246\tfile2 (esc)
  040000 tree 81b2a24c53f9090c6f6a23176a2a5660e6f48317\tsub2 (esc)
  100644 blob f7863ebb4c21391857fe5d27bc381553a2056223\tworkspace.josh (esc)
  $ tree
  .
  |-- file1
  |-- file2
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 4 files
