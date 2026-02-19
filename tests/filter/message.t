  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q testrepo 1> /dev/null
  $ cd testrepo

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "original message 1" 1> /dev/null

  $ echo contents2 > file2
  $ git add file2
  $ git commit -m "original message 2" 1> /dev/null

  $ echo contents3 > file3
  $ git add file3
  $ git commit -m "original message 3" 1> /dev/null

Test that message rewriting works
  $ josh-filter ':"new message"' --update refs/josh/filter/master master
  5f6f6e08a73a44279f4c80bd928430663c7ebbb2
  $ git log --pretty=%s josh/filter/master
  new message
  new message
  new message
  $ git log --pretty=%s master
  original message 3
  original message 2
  original message 1

  $ cd ${TESTTMP}
  $ git init -q testrepo2 1> /dev/null
  $ cd testrepo2

  $ echo contents1 > file1
  $ git add file1
  $ git commit -m "commit with {#} and {@}" 1> /dev/null

Test that message rewriting with template variables works
  $ josh-filter ':"Message: {#} {@}"' --update refs/josh/filter/master master
  1d858b36701f0d673e34f0f601a048b9c9c8d114
  $ git log --pretty=%s josh/filter/master
  Message: 3d77ff51363c9825cc2a221fc0ba5a883a1a2c72 2c0be119f4925350c097c9e206dfa6353158bba3
  $ git cat-file commit josh/filter/master | grep -A 1 "^$"
  
  Message: 3d77ff51363c9825cc2a221fc0ba5a883a1a2c72 2c0be119f4925350c097c9e206dfa6353158bba3

  $ cd ${TESTTMP}
  $ git init -q testrepo3 1> /dev/null
  $ cd testrepo3

  $ echo "file content" > file1
  $ mkdir -p subdir
  $ echo "nested content" > subdir/file2
  $ git add file1 subdir/file2
  $ git commit -m "initial commit" 1> /dev/null

Test that message rewriting with file content template variable works
  $ josh-filter ':"File content: {/file1}"' --update refs/josh/filter/master master
  cd7b44dc763fe78dc0b759398e689e54aa131eb5
  $ git log --pretty=%s josh/filter/master
  File content: file content
  $ git cat-file commit josh/filter/master | grep -A 1 "^$"
  
  File content: file content

Test that message rewriting with nested file path works
  $ josh-filter ':"Nested: {/subdir/file2}"' --update refs/josh/filter/master master
  23f3df907d06d6269adfc749e57b0c2974d66181
  $ git log --pretty=%s josh/filter/master
  Nested: nested content
  $ git cat-file commit josh/filter/master | grep -A 1 "^$"
  
  Nested: nested content

Test that message rewriting with tree entry OID works
  $ josh-filter ':"File OID: {#file1}"' --update refs/josh/filter/master master
  f90332f7fe886418042703808cca42bf1e33af7c
  $ git log --pretty=%s josh/filter/master | head -1
  File OID: * (glob)
  $ git cat-file commit josh/filter/master | grep -A 1 "^$" | head -1
  

Test that message rewriting with nested tree entry OID works
  $ josh-filter ':"Nested OID: {#subdir/file2}"' --update refs/josh/filter/master master
  7c6a0f3f4866f824e3d88a7d3277f85d2c1c62f5
  $ git log --pretty=%s josh/filter/master | head -1
  Nested OID: * (glob)
  $ git cat-file commit josh/filter/master | grep -A 1 "^$" | head -1
  

Test that non-existent file path returns empty content
  $ josh-filter ':"Missing: [{/nonexistent}]"' --update refs/josh/filter/master master
  8bf5b583555dd6c4765f3c34515de7e6c79813ac
  $ git log --pretty=%s josh/filter/master | head -1
  Missing: []
  $ git cat-file commit josh/filter/master | grep -A 1 "^$" | head -1
  

Test that non-existent tree entry returns zero OID
  $ josh-filter ':"Missing OID: {#nonexistent}"' --update refs/josh/filter/master master
  f63a6621696edc2b9ccec9a2ccd042af6276b081
  $ git log --pretty=%s josh/filter/master | head -1
  Missing OID: 0000000000000000000000000000000000000000
  $ git cat-file commit josh/filter/master | grep -A 1 "^$" | head -1
  

Test combining multiple template variables
  $ josh-filter ':"Tree: {#}, Commit: {@}, File: {/file1}, OID: {#file1}"' --update refs/josh/filter/master master
  5be71b6c02eb9a6aa6c1d4cd1fb2b682d732a940
  $ git log --pretty=%s josh/filter/master | head -1
  Tree: * (glob)
  $ git cat-file commit josh/filter/master | grep -A 1 "^$" | head -1
  

