  $ export TESTTMP=${PWD}

  $ cd ${TESTTMP}
  $ git init repo 1> /dev/null
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
  $ echo contents2 > sub1/file2
  $ echo contents3 > sub2/file3
  $ echo contents4 > sub3/sub4/file4
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ cat > sub1/x.graphql <<EOF
  > query {
  >  hash
  >  summary
  >  date(format: "%d.%m.%Y %H:%M:%S")
  >  config: file(path: "config_file.toml") {
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
  > {{ #with (graphql file="x.graphql") as |commit| }}
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
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter :/ HEAD -q get=sub1/file1
  contents1
  $ josh-filter :nop HEAD -q get=sub1/file2
  contents2
  $ josh-filter :/sub1 HEAD -q get=file1
  contents1

  $ josh-filter :/sub1

  $ josh-filter -q render=sub1/file1
  contents1
  $ josh-filter -q "graphql=sub1/x.graphql"
  {
    "hash": "9bf2c7be81ce7114a0af32c8dfe3667a7c5b37ce",
    "summary": "add file2",
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
  
  ID: 9bf2c7be81ce7114a0af32c8dfe3667a7c5b37ce
  Summary: add file2
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
  
  $ josh-filter :/sub1 -q render=file2
  contents2
