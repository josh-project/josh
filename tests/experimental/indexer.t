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
  899dcf292f1324dd6dc7847df15ef55419362675
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
  │   │   │   └── 8f
  │   │   ├── 6f/
  │   │   │   └── 8f
  │   │   └── 74/
  │   │       └── 8f
  │   ├── 64/
  │   │   └── 6f/
  │   │       └── 8f
  │   ├── 68/
  │   │   └── 61/
  │   │       └── 48
  │   ├── 6c/
  │   │   └── 69/
  │   │       └── 8f
  │   ├── 6d/
  │   │   └── 6f/
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 6f/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 6e/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 73/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  │   │       └── 48
  │   ├── 74/
  │   │   ├── 65/
  │   │   │   └── 8f
  │   │   ├── 68/
  │   │   │   └── 8f
  │   │   └── 6f/
  │   │       └── 48
  │   └── 77/
  │       ├── 68/
  │       │   └── 48
  │       └── 69/
  │           └── 8f
  ├── 61/
  │   ├── 6e/
  │   │   ├── 20/
  │   │   │   └── 8f
  │   │   └── 6f/
  │   │       └── 8f
  │   ├── 70/
  │   │   └── 70/
  │   │       └── 48
  │   └── 74/
  │       └── 20/
  │           └── 48
  ├── 63/
  │   └── 75/
  │       └── 6d/
  │           └── 8f
  ├── 64/
  │   └── 6f/
  │       └── 63/
  │           └── 8f
  ├── 65/
  │   ├── 20/
  │   │   ├── 20/
  │   │   │   └── 8f
  │   │   ├── 6c/
  │   │   │   └── 8f
  │   │   ├── 6d/
  │   │   │   └── 48
  │   │   ├── 74/
  │   │   │   └── 48
  │   │   └── 77/
  │   │       └── 48
  │   ├── 65/
  │   │   └── 20/
  │   │       └── 48
  │   ├── 6e/
  │   │   ├── 73/
  │   │   │   └── 48
  │   │   └── 74/
  │   │       └── 8f
  │   ├── 72/
  │   │   └── 20/
  │   │       └── 8f
  │   └── 73/
  │       └── 74/
  │           └── 8f
  ├── 66/
  │   └── 69/
  │       └── 72/
  │           └── 8f
  ├── 68/
  │   ├── 20/
  │   │   └── 6d/
  │   │       └── 8f
  │   ├── 61/
  │   │   ├── 6e/
  │   │   │   └── 8f
  │   │   ├── 70/
  │   │   │   └── 48
  │   │   └── 74/
  │   │       └── 48
  │   └── 65/
  │       └── 72/
  │           └── 8f
  ├── 69/
  │   ├── 6e/
  │   │   └── 65/
  │   │       └── 8f
  │   ├── 72/
  │   │   └── 73/
  │   │       └── 8f
  │   └── 74/
  │       └── 68/
  │           └── 8f
  ├── 6c/
  │   └── 69/
  │       └── 6e/
  │           └── 8f
  ├── 6d/
  │   ├── 65/
  │   │   └── 6e/
  │   │       └── 8f
  │   └── 6f/
  │       └── 72/
  \xe2\x94\x82           \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82           \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 6e/ (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 20/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 20/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 20/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 6f/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 74/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 74/ (escaped)
  \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 20/ (escaped)
  \xe2\x94\x82           \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 6f/ (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 20/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 73/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 63/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 75/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 6e/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 72/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 74/ (escaped)
  \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 68/ (escaped)
  \xe2\x94\x82           \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 70/ (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 6e/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 70/ (escaped)
  \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  │           └── 48
  ├── 72/
  │   ├── 20/
  │   │   └── 64/
  │   │       └── 8f
  │   ├── 65/
  │   │   └── 20/
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 48 (escaped)
  \xe2\x94\x82   \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 73/ (escaped)
  \xe2\x94\x82       \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 74/ (escaped)
  \xe2\x94\x82           \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 8f (escaped)
  \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 73/ (escaped)
  \xe2\x94\x82   \xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  \xe2\x94\x82   \xe2\x94\x82   \xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 65/ (escaped)
  │   │       └── 48
  │   └── 74/
  │       └── 20/
  │           └── 8f
  ├── 74/
  │   ├── 20/
  │   │   ├── 64/
  │   │   │   └── 8f
  │   │   ├── 68/
  │   │   │   └── 48
  │   │   ├── 74/
  │   │   │   └── 8f
  │   │   └── 77/
  │   │       └── 8f
  │   ├── 65/
  │   │   └── 73/
  │   │       └── 8f
  │   ├── 68/
  │   │   ├── 20/
  │   │   │   └── 8f
  │   │   ├── 61/
  │   │   │   └── 8f
  │   │   └── 65/
  │   │       └── 8f
  │   └── 6f/
  │       └── 20/
  │           └── 48
  ├── 75/
  │   └── 6d/
  │       └── 65/
  │           └── 8f
  └── 77/
      ├── 68/
      │   └── 61/
      │       └── 48
      └── 69/
          └── 74/
              └── 8f
