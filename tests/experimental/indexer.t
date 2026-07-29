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

  $ git diff ${EMPTY_TREE}..refs/heads/index
  diff --git a/01/04/8f b/01/04/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/03/3f/8f b/03/3f/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/04/25/8f b/04/25/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/07/28/48 b/07/28/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/09/33/8f b/09/33/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/0b/23/48 b/0b/23/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/0c/10/48 b/0c/10/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/0d/01/8f b/0d/01/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/0d/07/8f b/0d/07/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/10/3d/8f b/10/3d/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/11/0b/8f b/11/0b/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/11/21/8f b/11/21/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/11/2e/8f b/11/2e/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/11/33/8f b/11/33/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/12/23/48 b/12/23/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/13/16/8f b/13/16/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/13/3b/48 b/13/3b/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/13/3f/8f b/13/3f/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/15/24/48 b/15/24/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/16/18/8f b/16/18/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/17/18/8f b/17/18/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/17/35/8f b/17/35/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/17/38/8f b/17/38/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/18/04/48 b/18/04/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/18/1c/8f b/18/1c/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/18/23/48 b/18/23/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/19/15/8f b/19/15/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/19/3e/8f b/19/3e/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/1b/3e/48 b/1b/3e/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/1c/1b/48 b/1c/1b/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/1c/1b/8f b/1c/1b/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/1d/05/8f b/1d/05/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/1e/02/8f b/1e/02/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/13/8f b/20/13/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/21/02/48 b/21/02/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/21/20/48 b/21/20/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/23/1b/8f b/23/1b/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/23/29/8f b/23/29/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/23/3e/8f b/23/3e/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/24/1f/8f b/24/1f/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/24/25/48 b/24/25/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/24/25/8f b/24/25/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/25/2f/8f b/25/2f/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/26/31/8f b/26/31/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/27/08/8f b/27/08/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/28/17/48 b/28/17/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/29/30/8f b/29/30/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/2a/3d/8f b/2a/3d/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/2b/37/48 b/2b/37/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/2c/27/8f b/2c/27/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/2d/1d/48 b/2d/1d/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/2e/20/48 b/2e/20/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/2e/31/48 b/2e/31/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/30/04/8f b/30/04/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/30/09/8f b/30/09/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/30/10/8f b/30/10/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/33/0e/8f b/33/0e/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/33/18/8f b/33/18/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/34/02/48 b/34/02/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/34/02/8f b/34/02/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/34/2e/8f b/34/2e/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/35/3d/8f b/35/3d/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/36/13/48 b/36/13/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/36/13/8f b/36/13/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/38/19/8f b/38/19/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/38/1b/8f b/38/1b/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/39/0e/48 b/39/0e/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/39/0e/8f b/39/0e/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3a/3d/8f b/3a/3d/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3b/20/48 b/3b/20/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3b/20/8f b/3b/20/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3c/3d/48 b/3c/3d/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3d/07/8f b/3d/07/8f
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3e/05/48 b/3e/05/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3e/26/48 b/3e/26/48
  new file mode 100644
  index 0000000..e69de29
  diff --git a/3f/16/48 b/3f/16/48
  new file mode 100644
  index 0000000..e69de29
