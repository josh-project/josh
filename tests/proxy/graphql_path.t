  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}


  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ curl -s http://localhost:8002/version
  Version: 0.3.0

  $ cd real_repo

  $ git status
  On branch master
  
  No commits yet
  
  nothing to commit (create/copy files and use "git add" to track)

  $ git checkout -b master
  Switched to a new branch 'master'

  $ mkdir ws
  $ cat > ws/file1 <<EOF
  > content
  > EOF
  $ mkdir ws2
  $ cat > ws2/file2 <<EOF
  > content2
  > EOF

  $ git add ws*/*
  $ git commit -m "add file1" 1> /dev/null

  $ git push
  To http://localhost:8001/real_repo.git
   * [new branch]      master -> master

  $ cd ${TESTTMP}

  $ cat > ../query << EOF
  > {"query": "{
  >  rev(at:\"refs/heads/master\") {
  >    dir {
  >      path
  >      hash
  >    }
  >  }
  > }"}
  > EOF

  $ cat ../query | curl -s -X POST -H "content-type: application/json" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "dir": {
          "path": "",
          "hash": "fcd9aba1ef0c6d812452bdcb04ac155f5d7f42d6"
        }
      }
    }
  } (no-eol)

  $ cat ../query | curl -s -X POST -H "content-type: application/json; charset=utf-8" --data @- "http://localhost:8002/~/graphql/real_repo.git"
  {
    "data": {
      "rev": {
        "dir": {
          "path": "",
          "hash": "fcd9aba1ef0c6d812452bdcb04ac155f5d7f42d6"
        }
      }
    }
  } (no-eol)

$ cat ${TESTTMP}/josh-proxy.out
