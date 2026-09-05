  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q testrepo 1> /dev/null
  $ cd testrepo

  $ mkdir sub1
  $ printf "First Test document" > sub1/file1
  $ git add sub1
  $ git commit -m "add file1" 1> /dev/null

  $ printf "Another document with more \n than \n one line" > sub1/file2
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ mkdir sub2
  $ printf "One more to see what happens" > sub2/file3
  $ git add sub2
  $ git commit -m "add file3" 1> /dev/null

  $ josh-filter -s :INDEX --update refs/heads/index
  5c4b503ed540057ca9676d1ffb1cc54daa091f05
  [3] reachable_roots
  [3] sequence_number
  [6] :INDEX

  $ josh-filter :/ --search "Another"
  sub1/file2:1: Another document with more 
  2b1320977125dad24866056fa94acf30d77d9453
  $ josh-filter :/ --search "happens"
  sub2/file3:1: One more to see what happens
  2b1320977125dad24866056fa94acf30d77d9453
  $ josh-filter :/ --search "Test"
  sub1/file1:1: First Test document
  2b1320977125dad24866056fa94acf30d77d9453
  $ josh-filter :/ --search "document"
  sub1/file1:1: First Test document
  sub1/file2:1: Another document with more 
  2b1320977125dad24866056fa94acf30d77d9453
  $ josh-filter :/ --search "x"
  2b1320977125dad24866056fa94acf30d77d9453
  $ josh-filter :/ --search "e"
  sub1/file1:1: First Test document
  sub1/file2:1: Another document with more 
  sub1/file2:3:  one line
  sub2/file3:1: One more to see what happens
  2b1320977125dad24866056fa94acf30d77d9453
  $ josh-filter :/ --search "line"
  sub1/file2:3:  one line
  2b1320977125dad24866056fa94acf30d77d9453

  $ josh-filter :/ -g 'query { rev(at: "refs/heads/master") { results: search(string: "e") { path { path }, matches { line, text }} }}'
  2b1320977125dad24866056fa94acf30d77d9453
  {
    "rev": {
      "results": [
        {
          "path": {
            "path": "sub1/file1"
          },
          "matches": [
            {
              "line": 1,
              "text": "First Test document"
            }
          ]
        },
        {
          "path": {
            "path": "sub1/file2"
          },
          "matches": [
            {
              "line": 1,
              "text": "Another document with more "
            },
            {
              "line": 3,
              "text": " one line"
            }
          ]
        },
        {
          "path": {
            "path": "sub2/file3"
          },
          "matches": [
            {
              "line": 1,
              "text": "One more to see what happens"
            }
          ]
        }
      ]
    }
  }
  $ josh-filter :/ -g 'query { rev(at: "refs/heads/master", filter: ":/sub2") { results: search(string: "e") { path { path }, matches { line, text }} }}'
  2b1320977125dad24866056fa94acf30d77d9453
  {
    "rev": {
      "results": [
        {
          "path": {
            "path": "file3"
          },
          "matches": [
            {
              "line": 1,
              "text": "One more to see what happens"
            }
          ]
        }
      ]
    }
  }

  $ git-tree-pretty refs/heads/index
  .
  ├── 01/
  │   └── 04/
  │       └── 8f
  ├── 03/
  │   └── 3f/
  │       └── 8f
  ├── 04/
  │   └── 25/
  │       └── 8f
  ├── 07/
  │   └── 28/
  │       └── 48
  ├── 09/
  │   └── 33/
  │       └── 8f
  ├── 0b/
  │   └── 23/
  │       └── 48
  ├── 0c/
  │   └── 10/
  │       └── 48
  ├── 0d/
  │   ├── 01/
  │   │   └── 8f
  │   └── 07/
  │       └── 8f
  ├── 10/
  │   └── 3d/
  │       └── 8f
  ├── 11/
  │   ├── 0b/
  │   │   └── 8f
  │   ├── 21/
  │   │   └── 8f
  │   ├── 2e/
  │   │   └── 8f
  │   └── 33/
  │       └── 8f
  ├── 12/
  │   └── 23/
  │       └── 48
  ├── 13/
  │   ├── 16/
  │   │   └── 8f
  │   ├── 3b/
  │   │   └── 48
  │   └── 3f/
  │       └── 8f
  ├── 15/
  │   └── 24/
  │       └── 48
  ├── 16/
  │   └── 18/
  │       └── 8f
  ├── 17/
  │   ├── 18/
  │   │   └── 8f
  │   ├── 35/
  │   │   └── 8f
  │   └── 38/
  │       └── 8f
  ├── 18/
  │   ├── 04/
  │   │   └── 48
  │   ├── 1c/
  │   │   └── 8f
  │   └── 23/
  │       └── 48
  ├── 19/
  │   ├── 15/
  │   │   └── 8f
  │   └── 3e/
  │       └── 8f
  ├── 1b/
  │   └── 3e/
  │       └── 48
  ├── 1c/
  │   └── 1b/
  │       ├── 48
  │       └── 8f
  ├── 1d/
  │   └── 05/
  │       └── 8f
  ├── 1e/
  │   └── 02/
  │       └── 8f
  ├── 20/
  │   └── 13/
  │       └── 8f
  ├── 21/
  │   ├── 02/
  │   │   └── 48
  │   └── 20/
  │       └── 48
  ├── 23/
  │   ├── 1b/
  │   │   └── 8f
  │   ├── 29/
  │   │   └── 8f
  │   └── 3e/
  │       └── 8f
  ├── 24/
  │   ├── 1f/
  │   │   └── 8f
  │   └── 25/
  │       ├── 48
  │       └── 8f
  ├── 25/
  │   └── 2f/
  │       └── 8f
  ├── 26/
  │   └── 31/
  │       └── 8f
  ├── 27/
  │   └── 08/
  │       └── 8f
  ├── 28/
  │   └── 17/
  │       └── 48
  ├── 29/
  │   └── 30/
  │       └── 8f
  ├── 2a/
  │   └── 3d/
  │       └── 8f
  ├── 2b/
  │   └── 37/
  │       └── 48
  ├── 2c/
  │   └── 27/
  │       └── 8f
  ├── 2d/
  │   └── 1d/
  │       └── 48
  ├── 2e/
  │   ├── 20/
  │   │   └── 48
  │   └── 31/
  │       └── 48
  ├── 30/
  │   ├── 04/
  │   │   └── 8f
  │   ├── 09/
  │   │   └── 8f
  │   └── 10/
  │       └── 8f
  ├── 33/
  │   ├── 0e/
  │   │   └── 8f
  │   └── 18/
  │       └── 8f
  ├── 34/
  │   ├── 02/
  │   │   ├── 48
  │   │   └── 8f
  │   └── 2e/
  │       └── 8f
  ├── 35/
  │   └── 3d/
  │       └── 8f
  ├── 36/
  │   └── 13/
  │       ├── 48
  │       └── 8f
  ├── 38/
  │   ├── 19/
  │   │   └── 8f
  │   └── 1b/
  │       └── 8f
  ├── 39/
  │   └── 0e/
  │       ├── 48
  │       └── 8f
  ├── 3a/
  │   └── 3d/
  │       └── 8f
  ├── 3b/
  │   └── 20/
  │       ├── 48
  │       └── 8f
  ├── 3c/
  │   └── 3d/
  │       └── 48
  ├── 3d/
  │   └── 07/
  │       └── 8f
  ├── 3e/
  │   ├── 05/
  │   │   └── 48
  │   └── 26/
  │       └── 48
  └── 3f/
      └── 16/
          └── 48
