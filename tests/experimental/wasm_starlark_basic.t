  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo 1> /dev/null
  $ cd real_repo

  $ mkdir sub1 sub2
  $ echo contents1 > sub1/file1
  $ echo contents2 > sub2/file2
  $ git add sub1 sub2
  $ git commit -m "add subs" 1> /dev/null

Starlark scripts run through the interpreter guest blob, committed once as
tools/starlark.wasm and invoked with the script path as its argument.

  $ mkdir -p tools st
  $ cp "$(wasm-guest-blob.sh starlark)" tools/starlark.wasm
  $ cat > st/config.star <<'EOF'
  > # Simple starlark filter: keep only sub1
  > filter = filter.subdir("sub1")
  > EOF
  $ git add tools st
  $ git commit -m "add starlark config" 1> /dev/null

A later commit switches the script to select sub2 instead: the filter change
mid-history goes through the splicing machinery.

  $ cat > st/config.star <<'EOF'
  > filter = filter.subdir("sub2")
  > EOF
  $ git add st
  $ git commit -m "switch script to sub2" 1> /dev/null

  $ echo changed > sub2/file2
  $ git add sub2
  $ git commit -m "change file2" 1> /dev/null

The interpreter module comes from the input tree; the script comes from the
context tree (::st/ includes it). Commits predating the interpreter and
script filter to empty, and the script change splices in the history of the
newly selected sub2 as a synthetic merge — the projection graph stays
connected.

  $ josh-filter -s ':!tools/starlark=st/config.star[::st/]' master --update refs/josh/master
  f9a93f625cd211de2da73963f3cd2fdaa1359687
  [1] :[
      ::st/
      :/sub1
  ]
  [1] :subtract[
          :/sub2
          :/sub1
      ]
  [4] :~(
      josh-wasm-limits="fuel=100000000,memory=67108864,module_size=16777216,handles=100000,depth=10"
  )[
      :!tools/starlark=st/config.star[::st/]
  ]
  [4] reachable_roots
  [4] sequence_number

  $ git log --graph --pretty=%s refs/josh/master
  * change file2
  *   switch script to sub2
  |\  
  | * add subs
  * add starlark config
  * add subs

  $ git ls-tree -r refs/josh/master --name-only
  file2
  st/config.star

  $ git ls-tree -r refs/josh/master~2 --name-only
  file1
  st/config.star
