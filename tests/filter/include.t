  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

--------
1) Setup
--------

  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents4 > sub1/file4
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ echo contents3 > file3
  $ git add file3
  $ git commit -m "add file3" 1> /dev/null

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

---------------
2) include file
---------------

  $ mkdir f
  $ cat > f/include.josh <<EOF
  > :/sub1::file1
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ git add f/include.josh
  $ git commit -m "add include file" 1> /dev/null

  $ josh-filter -s :include=f/include.josh master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] ::file1
  [1] :[
      ::file1
      :prefix=a
  ]
  [1] :prefix=a
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] :[
      :/sub1:[
          ::file1
          :prefix=a
      ]
      ::sub2/subsub/
  ]
  [2] :include=f/include.josh

  $ git log --graph --pretty=%s refs/josh/master
  * add include file
  * add file2
  * add file1

  $ git checkout refs/josh/master 2> /dev/null
  $ tree
  .
  |-- a
  |   `-- file4
  |-- f
  |   `-- include.josh
  |-- file1
  `-- sub2
      `-- subsub
          `-- file2
  
  4 directories, 4 files

----------------------------------------
3) workspace file including include file
----------------------------------------

  $ git checkout master 2> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh << EOF
  > :include=f/include.josh
  > ::file3
  > EOF

  $ git add ws/workspace.josh

  $ git commit -m "add ws"
  [master e1ff4ff] add ws
   1 file changed, 2 insertions(+)
   create mode 100644 ws/workspace.josh

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] ::file1
  [1] :[
      ::file1
      :prefix=a
  ]
  [1] :prefix=a
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] ::file3
  [2] :[
      :/sub1:[
          ::file1
          :prefix=a
      ]
      ::sub2/subsub/
  ]
  [2] :include=f/include.josh
  [2] :workspace=ws
  [3] :[
      :include=f/include.josh
      ::file3
  ]

  $ git checkout refs/josh/master 2> /dev/null

  $ tree
  .
  |-- a
  |   `-- file4
  |-- f
  |   `-- include.josh
  |-- file1
  |-- file3
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  4 directories, 6 files

  $ git log --graph --oneline
  * 3b918e7 add ws
  * b562c24 add include file
  * 7c30b7a add file3

----------------------------------------
4) Edit include file
----------------------------------------

  $ git checkout master 2>/dev/null

Remove :/a from include file

  $ cat > f/include.josh <<EOF
  > :/sub1::file1
  > ::sub2/subsub/
  > EOF

  $ git add f/include.josh

  $ git commit -m "Edit include file"
  [master 57f1865] Edit include file
   1 file changed, 1 deletion(-)

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/sub1
  [1] :/subsub
  [1] ::file1
  [1] :[
      ::file1
      :prefix=a
  ]
  [1] :prefix=a
  [1] :prefix=sub2
  [1] :prefix=subsub
  [2] :/sub2
  [2] ::file3
  [2] :[
      :/sub1:[
          ::file1
          :prefix=a
      ]
      ::sub2/subsub/
  ]
  [2] :include=f/include.josh
  [3] :[
      :include=f/include.josh
      ::file3
  ]
  [3] :workspace=ws

  $ git checkout refs/josh/master 2>/dev/null

  $ tree
  .
  |-- f
  |   `-- include.josh
  |-- file1
  |-- file3
  |-- sub2
  |   `-- subsub
  |       `-- file2
  `-- workspace.josh
  
  3 directories, 5 files

  $ git log --graph --oneline
  * 586aad7 Edit include file
  * 3b918e7 add ws
  * b562c24 add include file
  * 7c30b7a add file3
