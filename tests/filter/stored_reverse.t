  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1
  $ echo contents1 > sub1/file1
  $ echo contents4 > sub1/file4
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ mkdir -p sub2/subsub
  $ echo contents1 > sub2/subsub/file2
  $ git add sub2
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir st
  $ cat > st/config.josh <<EOF
  > ::sub2/subsub/
  > a = :/sub1
  > EOF
  $ git add st
  $ git commit -m "add st" 1> /dev/null

  $ josh-filter -s :+st/config master --update refs/heads/filtered
  [2] :+st/config
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] sequence_number

  $ git log --graph --pretty=%s refs/heads/filtered
  * add st
  * add file2
  * add file1

  $ git checkout filtered
  Switched to branch 'filtered'
  $ tree
  .
  |-- a
  |   |-- file1
  |   `-- file4
  |-- st
  |   `-- config.josh
  `-- sub2
      `-- subsub
          `-- file2
  
  5 directories, 4 files

  $ echo modified_content > a/file1
  $ echo new_file_content > new_file
  $ git add .
  $ git commit -m "modify and add files" 1> /dev/null

  $ josh-filter -s :+st/config --reverse master --update refs/heads/filtered
  [2] :+st/config
  [2] :[
      a = :/sub1
      ::sub2/subsub/
  ]
  [3] sequence_number
  $ git checkout master
  Switched to branch 'master'

  $ tree
  .
  |-- st
  |   `-- config.josh
  |-- sub1
  |   |-- file1
  |   `-- file4
  `-- sub2
      `-- subsub
          `-- file2
  
  5 directories, 4 files

  $ cat sub1/file1
  modified_content
  $ cat new_file
  cat: new_file: No such file or directory
  [1]

