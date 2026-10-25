Edit an overlaid file with fallback to original build scripts

  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo
  $ cd real_repo

  $ mkdir -p mods/patched mods/fallback overlay/mods/patched st
  $ echo original-build > mods/patched/build.gradle
  $ echo fallback-build > mods/fallback/build.gradle
  $ echo keep > mods/patched/keep.txt
  $ echo overlay-build > overlay/mods/patched/build.gradle
  $ cat > st/config.josh <<EOF
  > :[
  >   ::mods/:exclude[::**/build.gradle]
  >   mods=:[
  >     :/mods::**/build.gradle
  >     :/overlay/mods
  >   ]
  > ]
  > EOF
  $ git add .
  $ git commit -q -m "add overlaid files"

  $ josh-filter :+st/config master --update refs/heads/filtered > /dev/null
  $ git checkout -q filtered
  $ git show HEAD:mods/patched/build.gradle
  overlay-build
  $ git show HEAD:mods/fallback/build.gradle
  fallback-build
  $ git show HEAD:mods/patched/keep.txt
  keep

  $ echo edited > mods/patched/build.gradle
  $ git commit -qam "edit overlaid build file"
  $ josh-filter :+st/config master --update refs/heads/filtered --reverse > /dev/null
  $ josh-filter :+st/config master --update refs/heads/refiltered > /dev/null
  $ test "$(git rev-parse refs/heads/filtered)" = "$(git rev-parse refs/heads/refiltered)"

Only the source file in the overlay should change

  $ git diff --name-only master^ master
  overlay/mods/patched/build.gradle
  $ git show master:overlay/mods/patched/build.gradle
  edited
  $ git show master:mods/patched/build.gradle
  original-build
  $ git show master:mods/fallback/build.gradle
  fallback-build
  $ git show master:mods/patched/keep.txt
  keep
