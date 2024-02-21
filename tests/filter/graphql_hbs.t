  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init -q repo 1> /dev/null
  $ cd repo

  $ echo contents0 > file0
  $ cat > config_file.toml <<EOF
  > [a]
  > b = "my_value"
  > EOF
  $ mkdir sub1
  $ mkdir sub2
  $ mkdir -p sub3/sub4
  $ echo contents1 > sub1/file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null
  $ echo contents2 > sub1/file2
  $ git add .
  $ git commit -m "add file2" 1> /dev/null
  $ echo contents3 > sub2/file3
  $ git add .
  $ git commit -m "add file3" 1> /dev/null
  $ echo contents4 > sub3/sub4/file4
  $ git add .
  $ git commit -m "add file4" 1> /dev/null

  $ cat > sub1/x.graphql <<EOF
  > query(\$name: String!) {
  >  hash
  >  summary
  >   history(limit: 100) {
  >     summary
  >   }
  >   history_default: history {
  >     summary
  >   }
  >   history_offset: history(limit: 2, offset: 2) {
  >     summary
  >   }
  >  date(format: "%d.%m.%Y %H:%M:%S")
  >  config: file(path: \$name) {
  >   data: toml {
  >    b: string(at: "/a/b")
  >    x: string(at: "/a/x")
  >   }
  >  }
  >  glob: rev(filter: "::**/file*") {
  >   files {
  >    path
  >    hash
  >    parent: dir(relative: "..") {
  >      path
  >    }
  >   }
  >   f1: files(depth: 1) {
  >    path
  >    hash
  >    parent: dir(relative: "..") {
  >      path
  >    }
  >   }
  >   f2: files(depth: 2) {
  >    path
  >    hash
  >    parent: dir(relative: "..") {
  >      path
  >    }
  >   }
  >   dirs {
  >    path
  >    hash
  >    parent: dir(relative: "..") {
  >      path
  >    }
  >   }
  >   d1: dirs(depth: 1) {
  >    path
  >    hash
  >    parent: dir(relative: "..") {
  >      path
  >    }
  >   }
  > }
  > }
  > EOF

  $ cat > sub1/tmpl_file <<EOF
  > tmpl_param1: {{ tmpl_param1 }}
  > tmpl_p2: {{ tmpl_p2 }}
  > {{ #with (graphql file="x.graphql" name="config_file.toml") as |commit| }}
  > ID: {{ commit.hash }}
  > Summary: {{ commit.summary }}
  > From TOML: {{ commit.config.data.b }}
  > From TOML: {{ commit.config.data.x }}
  > {{ #each commit.glob.files }}
  > path: {{ this.path }}
  > parent: {{ this.parent.path }}
  > sha1: {{ this.hash }}
  > {{ /each~}}
  > history:
  > {{ #each commit.history }}
  > - {{ this.summary }}
  > {{ /each~}}
  > {{ /with }}
  > EOF
  $ cat > sub1/tmpl_file_err <<EOF
  > tmpl_param1: {{ tmpl_param12 }}
  > tmpl_p2: {{ tmpl_p22 }}
  > {{ #with (graphql file="x.graphql" name="config_file.toml") as |commit| }}
  > ID: {{ commit.hash }}
  > Summary: {{ commit.summary }}
  > From TOML: {{ commit.config.data.b }}
  > From TOML: {{ commit.config.data.x }}
  > {{ #each commit.glob.files }}
  > path: {{ this.path }}
  > parent: {{ this.parent.path }}
  > sha1: {{ this.hash }}
  > {{ /each~}}
  > {{ /with }}
  > EOF
  $ git add sub1
  $ git commit -m "add templ_file" 1> /dev/null

  $ josh-filter :/ HEAD -q get=sub1/file1
  contents1
  $ josh-filter :nop HEAD -q get=sub1/file2
  contents2
  $ josh-filter :/sub1 HEAD -q get=file1
  contents1

  $ josh-filter :/sub1

  $ josh-filter -q render=sub1/file1
  contents1
  $ josh-filter -q "graphql=sub1/x.graphql&name=config_file.toml"
  {
    "hash": "a00263b0ee48ce1badf88d178a1e4fc27546aad0",
    "summary": "add templ_file",
    "history": [
      {
        "summary": "add templ_file"
      },
      {
        "summary": "add file4"
      },
      {
        "summary": "add file3"
      },
      {
        "summary": "add file2"
      },
      {
        "summary": "add file1"
      }
    ],
    "history_default": [
      {
        "summary": "add templ_file"
      }
    ],
    "history_offset": [
      {
        "summary": "add file3"
      },
      {
        "summary": "add file2"
      }
    ],
    "date": "07.04.2005 22:13:13",
    "config": {
      "data": {
        "b": "my_value",
        "x": null
      }
    },
    "glob": {
      "files": [
        {
          "path": "file0",
          "hash": "f25320b9e3f1dd09d15e6e13796402768d6d62cf",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub1/file1",
          "hash": "a024003ee1acc6bf70318a46e7b6df651b9dc246",
          "parent": {
            "path": "sub1"
          }
        },
        {
          "path": "sub1/file2",
          "hash": "6b46faacade805991bcaea19382c9d941828ce80",
          "parent": {
            "path": "sub1"
          }
        },
        {
          "path": "sub2/file3",
          "hash": "1cb5d64cdb55e3db2a8d6f00d596572b4cfa9d5c",
          "parent": {
            "path": "sub2"
          }
        },
        {
          "path": "sub3/sub4/file4",
          "hash": "288746e9035732a1fe600ee331de94e70f9639cb",
          "parent": {
            "path": "sub3/sub4"
          }
        }
      ],
      "f1": [
        {
          "path": "file0",
          "hash": "f25320b9e3f1dd09d15e6e13796402768d6d62cf",
          "parent": {
            "path": ""
          }
        }
      ],
      "f2": [
        {
          "path": "file0",
          "hash": "f25320b9e3f1dd09d15e6e13796402768d6d62cf",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub1/file1",
          "hash": "a024003ee1acc6bf70318a46e7b6df651b9dc246",
          "parent": {
            "path": "sub1"
          }
        },
        {
          "path": "sub1/file2",
          "hash": "6b46faacade805991bcaea19382c9d941828ce80",
          "parent": {
            "path": "sub1"
          }
        },
        {
          "path": "sub2/file3",
          "hash": "1cb5d64cdb55e3db2a8d6f00d596572b4cfa9d5c",
          "parent": {
            "path": "sub2"
          }
        }
      ],
      "dirs": [
        {
          "path": "sub1",
          "hash": "c627a2e3a6bfbb7307f522ad94fdfc8c20b92967",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub2",
          "hash": "2af8fd9cc75470c09c6442895133a815806018fc",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub3",
          "hash": "50207be2e0fadfbe2ca8d5e0616a71e7ec01f3e2",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub3/sub4",
          "hash": "883b1bd99f9c48cec992469c1ec20d2d3ea4bec0",
          "parent": {
            "path": "sub3"
          }
        }
      ],
      "d1": [
        {
          "path": "sub1",
          "hash": "c627a2e3a6bfbb7307f522ad94fdfc8c20b92967",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub2",
          "hash": "2af8fd9cc75470c09c6442895133a815806018fc",
          "parent": {
            "path": ""
          }
        },
        {
          "path": "sub3",
          "hash": "50207be2e0fadfbe2ca8d5e0616a71e7ec01f3e2",
          "parent": {
            "path": ""
          }
        }
      ]
    }
  } (no-eol)
  $ josh-filter -q "render=sub1/tmpl_file&tmpl_param1=tmpl_param_value1&tmpl_p2=val2"
  tmpl_param1: tmpl_param_value1
  tmpl_p2: val2
  ID: a00263b0ee48ce1badf88d178a1e4fc27546aad0
  Summary: add templ_file
  From TOML: my_value
  From TOML: 
  path: file0
  parent: 
  sha1: f25320b9e3f1dd09d15e6e13796402768d6d62cf
  path: sub1/file1
  parent: sub1
  sha1: a024003ee1acc6bf70318a46e7b6df651b9dc246
  path: sub1/file2
  parent: sub1
  sha1: 6b46faacade805991bcaea19382c9d941828ce80
  path: sub2/file3
  parent: sub2
  sha1: 1cb5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  path: sub3/sub4/file4
  parent: sub3/sub4
  sha1: 288746e9035732a1fe600ee331de94e70f9639cb
  history:
  - add templ_file
  - add file4
  - add file3
  - add file2
  - add file1
  $ josh-filter -q "render=sub1/tmpl_file_err&tmpl_param1=tmpl_param_value1&tmpl_p2=val2"
  ERROR: Error rendering "sub1/tmpl_file_err" line 1, col 14: Failed to access variable in strict mode Some("tmpl_param12")
  [1]
  $ josh-filter :/sub1 -q render=file2
  contents2
