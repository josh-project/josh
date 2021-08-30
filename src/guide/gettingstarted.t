  $ export TESTTMP=${PWD}
  $ mkdir git_data

# starting josh
ANCHOR: docker_github
  $ docker run -d -p 8000:8000 -e JOSH_REMOTE=https://github.com -v josh-vol:$(pwd)/git_data joshproject/josh-proxy:latest > josh.out
ANCHOR_END: docker_github

# waiting for josh to be running
  $ until curl -s http://localhost:8000/
  > do
  >     sleep 0.1
  > done

# cloning josh
ANCHOR: clone_full
  $ git clone http://localhost:8000/josh-project/josh.git
  Cloning into 'josh'...
  $ cd josh
ANCHOR_END: clone_full

# checking out a release tag to make the output dependable
  $ git checkout r21.03.19.1 1>/dev/null 2>/dev/null
ANCHOR: ls_full
  $ ls
  Cargo.lock
  Cargo.toml
  Dockerfile
  Dockerfile.tests
  LICENSE
  Makefile
  README.md
  docs
  josh-proxy
  run-josh.sh
  run-tests.sh
  rustfmt.toml
  scripts
  src
  static
  tests
  $ git log -2
  commit fc6af1e10c865f790bff7135d02b1fa82ddebe29
  Author: Christian Schilling <christian.schilling@esrlabs.com>
  Date:   Fri Mar 19 11:15:57 2021 +0100
  
      Update release.yml
  
  commit 975581064fa21b3a3d6871a4e888fd6dc1129a13
  Author: Christian Schilling <christian.schilling@esrlabs.com>
  Date:   Fri Mar 19 11:11:45 2021 +0100
  
      Update release.yml
ANCHOR_END: ls_full

# cloning doc
ANCHOR: clone_doc
  $ cd ..
  $ git clone http://localhost:8000/josh-project/josh.git:/docs.git
  Cloning into 'docs'...
  $ cd docs
ANCHOR_END: clone_doc

# checking out a release tag to make the output dependable
  $ git checkout r21.03.19.1 1>/dev/null 2>/dev/null
ANCHOR: ls_doc
  $ ls
  book.toml
  src
  $ git log -2
  commit dd26c506f6d6a218903b9f42a4869184fbbeb940
  Author: Christian Schilling <christian.schilling@esrlabs.com>
  Date:   Mon Mar 8 09:22:21 2021 +0100
  
      Update docs to use docker for default setup
  
  commit ee6abba0fed9b99c9426f5224ff93cfee2813edc
  Author: Louis-Marie Givel <louis-marie.givel@esrlabs.com>
  Date:   Fri Feb 26 11:41:37 2021 +0100
  
      Update proxy.md
ANCHOR_END: ls_doc

# cleanup
  $ cd ${TESTTMP}
  $ docker stop $(cat josh.out) >/dev/null
