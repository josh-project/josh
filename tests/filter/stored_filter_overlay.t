Edit an overlaid file through a stored filter

  $ export TERM=dumb
  $ export RUST_LOG_STYLE=never

  $ git init -q real_repo
  $ cd real_repo

  $ mkdir -p mods overlay/mods st
  $ echo original-build > mods/build.gradle
  $ echo original-settings > mods/settings.gradle
  $ echo overlay-build > overlay/mods/build.gradle
  $ echo overlay-settings > overlay/mods/settings.gradle
  $ cat > st/config.josh <<EOF
  > mods=:[
  >   :/mods::*.gradle
  >   :/overlay/mods
  > ]
  > EOF
  $ git add .
  $ git commit -q -m "add overlaid files"

  $ josh-filter :+st/config master --update refs/heads/filtered > /dev/null
  $ git checkout -q filtered
  $ git show HEAD:mods/build.gradle
  overlay-build
  $ git show HEAD:mods/settings.gradle
  overlay-settings

  $ echo edited > mods/build.gradle
  $ git commit -qam "edit overlaid build file"
  $ josh-filter :+st/config master --update refs/heads/filtered --reverse > /dev/null

Only the source file in the overlay should change

  $ git diff --name-only master^ master
  overlay/mods/build.gradle
  $ git show master:overlay/mods/build.gradle
  edited
  $ git show master:mods/build.gradle
  original-build
  $ git show master:mods/settings.gradle
  original-settings
