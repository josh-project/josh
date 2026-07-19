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
  ce00c6cd65dea70a35eeaa9283741bcd3f901991
  [3] :INDEX
  [3] reachable_roots
  [3] sequence_number
  [6] _trigram_index

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
  diff --git a/0a/20/6f/sub1/file2 b/0a/20/6f/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/0a/20/74/sub1/file2 b/0a/20/74/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/0a/20/sub1/file2 b/20/0a/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/54/65/sub1/file1 b/20/54/65/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/64/6f/sub1/file1 b/20/64/6f/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/64/6f/sub1/file2 b/20/64/6f/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/68/61/sub2/file3 b/20/68/61/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/6c/69/sub1/file2 b/20/6c/69/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/6d/6f/sub1/file2 b/20/6d/6f/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/6d/6f/sub2/file3 b/20/6d/6f/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/6f/6e/sub1/file2 b/20/6f/6e/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/73/65/sub2/file3 b/20/73/65/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/74/68/sub1/file2 b/20/74/68/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/74/6f/sub2/file3 b/20/74/6f/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/77/68/sub2/file3 b/20/77/68/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/20/77/69/sub1/file2 b/20/77/69/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/41/6e/6f/sub1/file2 b/41/6e/6f/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/46/69/72/sub1/file1 b/46/69/72/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/4f/6e/65/sub2/file3 b/4f/6e/65/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/54/65/73/sub1/file1 b/54/65/73/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/61/6e/20/sub1/file2 b/61/6e/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/61/70/70/sub2/file3 b/61/70/70/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/61/74/20/sub2/file3 b/61/74/20/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/63/75/6d/sub1/file1 b/63/75/6d/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/63/75/6d/sub1/file2 b/63/75/6d/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/64/6f/63/sub1/file1 b/64/6f/63/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/64/6f/63/sub1/file2 b/64/6f/63/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/20/0a/sub1/file2 b/65/20/0a/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/20/6c/sub1/file2 b/65/20/6c/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/20/6d/sub2/file3 b/65/20/6d/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/20/74/sub2/file3 b/65/20/74/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/20/77/sub2/file3 b/65/20/77/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/65/20/sub2/file3 b/65/65/20/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/6e/73/sub2/file3 b/65/6e/73/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/6e/74/sub1/file1 b/65/6e/74/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/6e/74/sub1/file2 b/65/6e/74/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/72/20/sub1/file2 b/65/72/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/65/73/74/sub1/file1 b/65/73/74/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/68/20/6d/sub1/file2 b/68/20/6d/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/68/61/6e/sub1/file2 b/68/61/6e/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/68/61/70/sub2/file3 b/68/61/70/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/68/61/74/sub2/file3 b/68/61/74/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/68/65/72/sub1/file2 b/68/65/72/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/69/6e/65/sub1/file2 b/69/6e/65/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/69/72/73/sub1/file1 b/69/72/73/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/69/74/68/sub1/file2 b/69/74/68/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6c/69/6e/sub1/file2 b/6c/69/6e/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6d/65/6e/sub1/file1 b/6d/65/6e/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6d/65/6e/sub1/file2 b/6d/65/6e/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6d/6f/72/sub1/file2 b/6d/6f/72/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6d/6f/72/sub2/file3 b/6d/6f/72/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6e/20/0a/sub1/file2 b/6e/20/0a/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6e/65/20/sub1/file2 b/6e/65/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6e/65/20/sub2/file3 b/6e/65/20/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6e/6f/74/sub1/file2 b/6e/6f/74/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6e/74/20/sub1/file2 b/6e/74/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/20/73/sub2/file3 b/6f/20/73/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/63/75/sub1/file1 b/6f/63/75/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/63/75/sub1/file2 b/6f/63/75/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/6e/65/sub1/file2 b/6f/6e/65/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/72/65/sub1/file2 b/6f/72/65/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/72/65/sub2/file3 b/6f/72/65/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/6f/74/68/sub1/file2 b/6f/74/68/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/70/65/6e/sub2/file3 b/70/65/6e/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/70/70/65/sub2/file3 b/70/70/65/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/72/20/64/sub1/file2 b/72/20/64/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/72/65/20/sub1/file2 b/72/65/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/72/65/20/sub2/file3 b/72/65/20/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/72/73/74/sub1/file1 b/72/73/74/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/73/65/65/sub2/file3 b/73/65/65/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/73/74/20/sub1/file1 b/73/74/20/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/20/54/sub1/file1 b/74/20/54/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/20/64/sub1/file1 b/74/20/64/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/20/68/sub2/file3 b/74/20/68/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/20/77/sub1/file2 b/74/20/77/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/68/20/sub1/file2 b/74/68/20/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/68/61/sub1/file2 b/74/68/61/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/68/65/sub1/file2 b/74/68/65/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/74/6f/20/sub2/file3 b/74/6f/20/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/75/6d/65/sub1/file1 b/75/6d/65/sub1/file1
  new file mode 100644
  index 0000000..e69de29
  diff --git a/75/6d/65/sub1/file2 b/75/6d/65/sub1/file2
  new file mode 100644
  index 0000000..e69de29
  diff --git a/77/68/61/sub2/file3 b/77/68/61/sub2/file3
  new file mode 100644
  index 0000000..e69de29
  diff --git a/77/69/74/sub1/file2 b/77/69/74/sub1/file2
  new file mode 100644
  index 0000000..e69de29
