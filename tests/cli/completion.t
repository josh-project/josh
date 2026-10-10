  $ export TESTTMP=${PWD}

  $ mkdir completion
  $ cd completion
  $ git init -q
  $ mkdir -p docs ws specs
  $ echo documentation > docs/guide.txt
  $ echo :/docs > ws/test.josh
  $ echo 'filter = None' > ws/build.star
  $ echo :/ > specs/filter.josh
  $ git add .
  $ git commit -qm "completion fixtures"
  $ git branch topic/test
  $ git tag v1
  $ echo :/docs > ws/uncommitted.josh

Both binaries emit Bash registration scripts:

  $ COMPLETE=bash josh | sed -n '/^_clap_complete_josh() {/p'
  _clap_complete_josh() {
  $ COMPLETE=bash josh-filter | sed -n '/^_clap_complete_josh_filter() {/p'
  _clap_complete_josh_filter() {

Zsh registration initializes its completion system before calling `compdef`:

  $ COMPLETE=zsh josh-filter | sed -n '/autoload -Uz compinit/p'
    autoload -Uz compinit

Static command completion comes from the josh command tree:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=1 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh -- josh co
  compose (no-eol)

Filter syntax is available even when the filter applies to a remote repository:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=3 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh -- josh clone example :wor
  :workspace= (no-eol)

Local filter completion reads the selected snapshot. Compose defaults to the working tree, while josh-filter defaults to HEAD:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=3 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh -- josh compose run :+ws/unc
  :+ws/uncommitted (no-eol)
  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=1 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter :/do
  :/docs/ (no-eol)
  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=1 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter ':exclude[:/do'
  :exclude[:/docs/ (no-eol)
  $ env COMPLETE=zsh _CLAP_COMPLETE_INDEX=1 josh-filter -- josh-filter ':+ws'
  :+ws/ (no-eol)

Starlark scripts complete without requiring the experimental feature flag in the
completion process. Remote filter arguments use the local tree when available:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=1 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter ':!ws/bu'
  :!ws/build (no-eol)
  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=3 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh -- josh clone example ':!ws/bu'
  :!ws/build (no-eol)
  $ env COMPLETE=zsh _CLAP_COMPLETE_INDEX=1 josh-filter -- josh-filter ':\!ws/bu'
  :\!ws/build (no-eol)

Revision completion supports branches and preserves revision suffixes:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=4 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh -- josh compose run --revision top
  topic/test (no-eol)
  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=2 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter :/ 'top~2'
  topic/test~2 (no-eol)

Refspec and writable-ref completion use full ref names:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=3 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter :/ --revspec refs/heads/top
  refs/heads/topic/test (no-eol)
  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=3 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter :/ --update refs/heads/top
  refs/heads/topic/test (no-eol)

Ordinary file arguments retain filesystem completion:

  $ env COMPLETE=bash _CLAP_COMPLETE_INDEX=2 _CLAP_COMPLETE_COMP_TYPE=9 _CLAP_COMPLETE_SPACE=false josh-filter -- josh-filter --file specs/fi
  specs/filter.josh (no-eol)
