  $ export TESTTMP=${PWD}
  $ export PATH=${TESTDIR}/../../target/debug/:${PATH}

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
  > {{ #each (git-find glob="**/file*") }}
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
  > { "$(git log -n1 --pretty="%H" JOSH_HEAD)" : "SUCCESS" }
  > EOF

  $ cat .git/josh_kv.json
  { "*" : "SUCCESS" } (glob)

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
  sha1: * (glob)
  blob: contents0
  -----
  1:
  name: file1
  path: sub1/file1
  base: sub1
  sha1: * (glob)
  blob: contents1
  filtered: * (glob)
    -----
  2:
  name: file2
  path: sub1/file2
  base: sub1
  sha1: * (glob)
  blob: contents2
  filtered: * (glob)
    -----
  3:
  name: file3
  path: sub2/file3
  base: sub2
  sha1: * (glob)
  blob: contents3
  filtered: * (glob)
    
  $ josh-filter -s :/sub1 -q render=file2
  [2] :/sub1
  contents2
