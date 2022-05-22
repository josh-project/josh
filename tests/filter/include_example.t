  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

-----------------------------------------------
1) Setup a module with doc and src
-----------------------------------------------

  $ git init real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir module1
  $ mkdir module1/doc
  $ echo contents1 > module1/doc/file1
  $ git add module1/doc/file1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir module1/src
  $ echo contents2 > module1/src/file2
  $ git add module1/src/file2
  $ git commit -m "add file2" 1> /dev/null

-------------------------------------
2) include files for doc, and for src
-------------------------------------

  $ cat > module1/include_doc.josh <<EOF
  > ::module1/doc/
  > EOF
  $ git add module1/include_doc.josh
  $ git commit -m "add include doc file" 1> /dev/null

  $ cat > module1/include_src.josh <<EOF
  > ::module1/src/
  > EOF
  $ git add module1/include_src.josh
  $ git commit -m "add include src file" 1> /dev/null

----------------------------------------------
3) workspace file including src
----------------------------------------------

  $ git checkout master 2> /dev/null

  $ mkdir ws
  $ cat > ws/workspace.josh << EOF
  > deps = :[
  > :include=module1/include_src.josh
  > ]
  > EOF

  $ git add ws/workspace.josh

  $ git commit -m "add ws"
  [master 5c76da1] add ws
   1 file changed, 3 insertions(+)
   create mode 100644 ws/workspace.josh

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :prefix=module1
  [1] :prefix=src
  [2] :/src
  [2] :include=module1/include_src.josh
  [2] :prefix=deps
  [2] :workspace=ws
  [3] :/module1

  $ git checkout refs/josh/master 2> /dev/null

  $ tree
  .
  |-- deps
  |   `-- module1
  |       |-- include_src.josh
  |       `-- src
  |           `-- file2
  `-- workspace.josh
  
  3 directories, 3 files

  $ git log --graph --oneline
  * e255d4c add ws
  * d1da88e add include src file
  * 4581710 add file2

----------------------------------------------
4) workspace file now needs doc
----------------------------------------------

  $ git checkout master 2> /dev/null

  $ cat > ws/workspace.josh << EOF
  > deps = :[
  > :include=module1/include_src.josh
  > :include=module1/include_doc.josh
  > ]
  > EOF

  $ git add ws/workspace.josh

  $ git commit -m "update ws"
  [master 873b6d0] update ws
   1 file changed, 1 insertion(+)

  $ josh-filter -s :workspace=ws master --update refs/josh/master
  [1] :/doc
  [1] :prefix=doc
  [1] :prefix=src
  [2] :/src
  [2] :include=module1/include_doc.josh
  [2] :include=module1/include_src.josh
  [2] :prefix=module1
  [3] :/module1
  [3] :workspace=ws
  [4] :prefix=deps

  $ git checkout refs/josh/master 2> /dev/null

  $ tree
  .
  |-- deps
  |   `-- module1
  |       |-- doc
  |       |   `-- file1
  |       |-- include_doc.josh
  |       |-- include_src.josh
  |       `-- src
  |           `-- file2
  `-- workspace.josh
  
  4 directories, 5 files

  $ git log --graph --oneline
  *   c567916 update ws
  |\  
  | * fcce2eb add include doc file
  | * f15f408 add file1
  * e255d4c add ws
  * d1da88e add include src file
  * 4581710 add file2

