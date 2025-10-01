  $ git init -q 1> /dev/null

  $ echo a > f
  $ git add f
  $ git commit -m init 1> /dev/null
  $ git notes add -m ':/:prefix=sub' -f

  $ git notes add -m ':/:prefix=sub' -f
  Overwriting existing notes for object 9a2b74734b5d6fb4210585cf49c17045156cce5b

  $ echo a > f2
  $ git add f2
  $ git commit -m "add f2" 1> /dev/null
  $ git notes add -m ':/:prefix=sub2' -f


  $ josh-filter -s :hook=commits HEAD --update refs/josh/filtered
  [2] :hook="commits"

  $ git log --pretty=format:"* %h %s" --name-only refs/josh/filtered
  * 6adcad5 add f2
  sub2/f
  sub2/f2
  
  * ac8b28b init
  sub/f

