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

  $ josh-filter -s
  c0f12229f0a2169c7475977697584b0a273e7c29
  $ josh-filter -d -s
  c0f12229f0a2169c7475977697584b0a273e7c29
  [1] :/sub1
  [1] :/subsub
  [1] :prefix=sub1
  [1] :prefix=sub2
  [1] :prefix=subsub
  [1] :prefix=ws
  [1] :prefix=ws2
  [2] :/sub2
  [2] :/ws
  [2] :/ws2
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
  [9] sequence_number

  $ cat > workspace.josh <<EOF
  > :/sub1::file1
  > :/sub1::file2
  > ::sub2/subsub
  > EOF
  $ git add .
  $ git commit -m "add root ws" 1> /dev/null

  $ josh-filter -d -s
  2a5512a2902428612da6ae41b6d2e7a468aa56b1
  [1] :/sub1
  [1] :/subsub
  [1] :prefix=sub1
  [1] :prefix=sub2
  [1] :prefix=subsub
  [1] :prefix=ws
  [1] :prefix=ws2
  [2] :/sub2
  [2] :/ws
  [2] :/ws2
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
  [10] sequence_number
