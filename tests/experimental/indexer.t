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
  4740798fdfd3f243763aad91b2badafbf72ff9e2
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
  ├── 20/
  │   ├── 20/
  │   │   ├── 20/
  │   │   │   └── sub1
  │   │   ├── 6f/
  │   │   │   └── sub1
  │   │   └── 74/
  │   │       └── sub1
  │   ├── 64/
  │   │   └── 6f/
  │   │       └── sub1
  │   ├── 68/
  │   │   └── 61/
  │   │       └── sub2
  │   ├── 6c/
  │   │   └── 69/
  │   │       └── sub1
  │   ├── 6d/
  │   │   └── 6f/
  │   │       ├── sub1
  │   │       └── sub2
  │   ├── 6f/
  │   │   └── 6e/
  │   │       └── sub1
  │   ├── 73/
  │   │   └── 65/
  │   │       └── sub2
  │   ├── 74/
  │   │   ├── 65/
  │   │   │   └── sub1
  │   │   ├── 68/
  │   │   │   └── sub1
  │   │   └── 6f/
  │   │       └── sub2
  │   └── 77/
  │       ├── 68/
  │       │   └── sub2
  │       └── 69/
  │           └── sub1
  ├── 61/
  │   ├── 6e/
  │   │   ├── 20/
  │   │   │   └── sub1
  │   │   └── 6f/
  │   │       └── sub1
  │   ├── 70/
  │   │   └── 70/
  │   │       └── sub2
  │   └── 74/
  │       └── 20/
  │           └── sub2
  ├── 63/
  │   └── 75/
  │       └── 6d/
  │           └── sub1
  ├── 64/
  │   └── 6f/
  │       └── 63/
  │           └── sub1
  ├── 65/
  │   ├── 20/
  │   │   ├── 20/
  │   │   │   └── sub1
  │   │   ├── 6c/
  │   │   │   └── sub1
  │   │   ├── 6d/
  │   │   │   └── sub2
  │   │   ├── 74/
  │   │   │   └── sub2
  │   │   └── 77/
  │   │       └── sub2
  │   ├── 65/
  │   │   └── 20/
  │   │       └── sub2
  │   ├── 6e/
  │   │   ├── 73/
  │   │   │   └── sub2
  │   │   └── 74/
  │   │       └── sub1
  │   ├── 72/
  │   │   └── 20/
  │   │       └── sub1
  │   └── 73/
  │       └── 74/
  │           └── sub1
  ├── 66/
  │   └── 69/
  │       └── 72/
  │           └── sub1
  ├── 68/
  │   ├── 20/
  │   │   └── 6d/
  │   │       └── sub1
  │   ├── 61/
  │   │   ├── 6e/
  │   │   │   └── sub1
  │   │   ├── 70/
  │   │   │   └── sub2
  │   │   └── 74/
  │   │       └── sub2
  │   └── 65/
  │       └── 72/
  │           └── sub1
  ├── 69/
  │   ├── 6e/
  │   │   └── 65/
  │   │       └── sub1
  │   ├── 72/
  │   │   └── 73/
  │   │       └── sub1
  │   └── 74/
  │       └── 68/
  │           └── sub1
  ├── 6c/
  │   └── 69/
  │       └── 6e/
  │           └── sub1
  ├── 6d/
  │   ├── 65/
  │   │   └── 6e/
  │   │       └── sub1
  │   └── 6f/
  │       └── 72/
  │           ├── sub1
  │           └── sub2
  ├── 6e/
  │   ├── 20/
  │   │   └── 20/
  │   │       └── sub1
  │   ├── 65/
  │   │   └── 20/
  │   │       ├── sub1
  │   │       └── sub2
  │   ├── 6f/
  │   │   └── 74/
  │   │       └── sub1
  │   └── 74/
  │       └── 20/
  │           └── sub1
  ├── 6f/
  │   ├── 20/
  │   │   └── 73/
  │   │       └── sub2
  │   ├── 63/
  │   │   └── 75/
  │   │       └── sub1
  │   ├── 6e/
  │   │   └── 65/
  │   │       ├── sub1
  │   │       └── sub2
  │   ├── 72/
  │   │   └── 65/
  │   │       ├── sub1
  │   │       └── sub2
  │   └── 74/
  │       └── 68/
  │           └── sub1
  ├── 70/
  │   ├── 65/
  │   │   └── 6e/
  │   │       └── sub2
  │   └── 70/
  │       └── 65/
  │           └── sub2
  ├── 72/
  │   ├── 20/
  │   │   └── 64/
  │   │       └── sub1
  │   ├── 65/
  │   │   └── 20/
  │   │       ├── sub1
  │   │       └── sub2
  │   └── 73/
  │       └── 74/
  │           └── sub1
  ├── 73/
  │   ├── 65/
  │   │   └── 65/
  │   │       └── sub2
  │   └── 74/
  │       └── 20/
  │           └── sub1
  ├── 74/
  │   ├── 20/
  │   │   ├── 64/
  │   │   │   └── sub1
  │   │   ├── 68/
  │   │   │   └── sub2
  │   │   ├── 74/
  │   │   │   └── sub1
  │   │   └── 77/
  │   │       └── sub1
  │   ├── 65/
  │   │   └── 73/
  │   │       └── sub1
  │   ├── 68/
  │   │   ├── 20/
  │   │   │   └── sub1
  │   │   ├── 61/
  │   │   │   └── sub1
  │   │   └── 65/
  │   │       └── sub1
  │   └── 6f/
  │       └── 20/
  │           └── sub2
  ├── 75/
  │   └── 6d/
  │       └── 65/
  │           └── sub1
  └── 77/
      ├── 68/
      │   └── 61/
      │       └── sub2
      └── 69/
          └── 74/
              └── sub1
