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
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub1/file2
  $ echo contents3 > sub2/file3
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

  $ cat > sub1/x.graphql <<EOF
  > query {
  >  id
  >  summary
  >  config: file(path: "config_file.toml") {
  >   data: toml {
  >    b: string(at: "/a/b")
  >    x: string(at: "/a/x")
  >   }
  >  }
  >  glob: commit(filter: "::**/file*") {
  >   files {
  >    id
  >    path
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
  > ID: {{ commit.id }}
  > Summary: {{ commit.summary }}
  > From TOML: {{ commit.config.data.b }}
  > From TOML: {{ commit.config.data.x }}
  > {{ #each commit.glob.files }}
  > path: {{ this.path }}
  > parent: {{ this.parent.path }}
  > sha1: {{ this.id }}
  > {{ /each~}}
  > {{ /with }}
  > EOF
  $ git add sub1
  $ git commit -m "add file2" 1> /dev/null

  $ josh-filter -s :nop HEAD -q get=sub1/file1
  contents1
  $ josh-filter -s :nop HEAD -q get=sub1/file2
  contents2
  $ josh-filter -s :/sub1 HEAD -q get=file1
  [2] :/sub1
  contents1

  $ josh-filter -s :/sub1
  [2] :/sub1

  $ josh-filter -s -q render=sub1/file1
  [2] :/sub1
  contents1
  $ josh-filter -s -q "render=sub1/tmpl_file&tmpl_param1=tmpl_param_value1&tmpl_p2=val2"
  [2] :/sub1
  tmpl_param1: tmpl_param_value1
  tmpl_p2: val2
  
  ID: cb658a86acfdb09eaa0b68ef57ebf9da5e2c5b5e
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
  
  $ josh-filter -s :/sub1 -q render=file2
  [2] :/sub1
  contents2
