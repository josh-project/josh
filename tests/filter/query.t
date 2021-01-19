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

  $ cat > sub1/tmpl_file <<EOF
  > tmpl_param1: {{ tmpl_param1 }}
  > tmpl_p2: {{ tmpl_p2 }}
  > {{ #with (toml (git-blob path="config_file.toml")) }}
  > From TOML: {{ a.b }}
  > {{ /with }}
  > {{ #each (git-ls filter="::**/file*") }}
  > {{ ~@index}}:
  > name: {{ this.name }}
  > path: {{ this.path }}
  > base: {{ this.base }}
  > sha1: {{ this.sha1 }}
  > {{ ~#with (git-blob path=this.path) as |b| }}
  > blob: {{{ b }}}
  > {{ ~/with~ }}
  > {{ ~#if this.base }}
  >   {{ ~#with (josh-filter spec=(concat ":workspace=" this.base))~ }}
  > filtered: {{{ sha1 }}}
  >   {{ /with~ }}
  > {{ ~/if }}
  > {{ ~#unless @last }}-----{{ /unless }}
  > {{ /each~}}
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

  $ cat > .git/josh_kv.json <<EOF
  > { "$(git log -n1 --pretty="%H" FILTERED_HEAD)" : "SUCCESS" }
  > EOF

  $ cat .git/josh_kv.json
  { "6818b278a7218c052915e067f6f7d7890e8748ba" : "SUCCESS" }

  $ josh-filter -s -q render=sub1/file1
  [2] :/sub1
  contents1
  $ josh-filter -s -q "render=sub1/tmpl_file&tmpl_param1=tmpl_param_value1&tmpl_p2=val2"
  [2] :/sub1
  tmpl_param1: tmpl_param_value1
  tmpl_p2: val2
  
  From TOML: my_value
  
  0:
  name: file0
  path: file0
  base: 
  sha1: f25320b9e3f1dd09d15e6e13796402768d6d62cf
  blob: contents0
  -----
  1:
  name: file1
  path: sub1/file1
  base: sub1
  sha1: a024003ee1acc6bf70318a46e7b6df651b9dc246
  blob: contents1
  filtered: 6818b278a7218c052915e067f6f7d7890e8748ba
    -----
  2:
  name: file2
  path: sub1/file2
  base: sub1
  sha1: 6b46faacade805991bcaea19382c9d941828ce80
  blob: contents2
  filtered: 6818b278a7218c052915e067f6f7d7890e8748ba
    -----
  3:
  name: file3
  path: sub2/file3
  base: sub2
  sha1: 1cb5d64cdb55e3db2a8d6f00d596572b4cfa9d5c
  blob: contents3
  filtered: d05ad19276daffb2c8cff4078c72c339346e19c4
    
  $ josh-filter -s :/sub1 -q render=file2
  [2] :/sub1
  contents2
