  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ git clone -q http://localhost:8001/real_repo.git
  warning: You appear to have cloned an empty repository.

  $ cd real_repo
  $ git lfs install
  Updated Git hooks.
  Git LFS initialized.
  $ git lfs track "*.large"
  Tracking "*.large"

  $ git status
  On branch master
  
  No commits yet
  
  Untracked files:
    (use "git add <file>..." to include in what will be committed)
  \t.gitattributes (esc)
  
  nothing added to commit but untracked files present (use "git add" to track)

  $ mkdir sub1
  $ echo contents1 > sub1/file1.large
  $ git add sub1
  $ git commit -m "add file1"
  [master (root-commit) 086980a] add file1
   1 file changed, 3 insertions(+)
   create mode 100644 sub1/file1.large

  $ tree
  .
  `-- sub1
      `-- file1.large
  
  1 directory, 1 file

$ git config lfs.url http://127.0.0.1:9999/
  $ git config lfs.http://localhost:8001/real_repo.git/info/lfs.locksverify false

  $ git lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.2700114.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git lfs logs last
  git-lfs/3.3.0 (GitHub; linux amd64; go 1.19.5; git 91bac118)
  git version 2.38.1
  
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  
  Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  github.com/git-lfs/git-lfs/v3/errors.Errorf
  \tgithub.com/git-lfs/git-lfs/v3/errors/errors.go:69 (esc)
  github.com/git-lfs/git-lfs/v3/lfshttp.defaultError
  \tgithub.com/git-lfs/git-lfs/v3/lfshttp/errors.go:126 (esc)
  github.com/git-lfs/git-lfs/v3/lfshttp.(*Client).handleResponse
  \tgithub.com/git-lfs/git-lfs/v3/lfshttp/errors.go:52 (esc)
  github.com/git-lfs/git-lfs/v3/lfshttp.(*Client).DoWithRedirect
  \tgithub.com/git-lfs/git-lfs/v3/lfshttp/client.go:335 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).doWithCreds
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:104 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).doWithAuth
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:68 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).DoWithAuth
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:26 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).DoAPIRequestWithAuth
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:57 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*tqClient).Batch
  \tgithub.com/git-lfs/git-lfs/v3/tq/api.go:92 (esc)
  github.com/git-lfs/git-lfs/v3/tq.Batch
  \tgithub.com/git-lfs/git-lfs/v3/tq/api.go:43 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*TransferQueue).enqueueAndCollectRetriesFor
  \tgithub.com/git-lfs/git-lfs/v3/tq/transfer_queue.go:565 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*TransferQueue).collectBatches.func1
  \tgithub.com/git-lfs/git-lfs/v3/tq/transfer_queue.go:459 (esc)
  runtime.goexit
  \truntime/asm_amd64.s:1594 (esc)
  Fatal error
  github.com/git-lfs/git-lfs/v3/errors.newWrappedError
  \tgithub.com/git-lfs/git-lfs/v3/errors/types.go:229 (esc)
  github.com/git-lfs/git-lfs/v3/errors.NewFatalError
  \tgithub.com/git-lfs/git-lfs/v3/errors/types.go:273 (esc)
  github.com/git-lfs/git-lfs/v3/lfshttp.(*Client).handleResponse
  \tgithub.com/git-lfs/git-lfs/v3/lfshttp/errors.go:76 (esc)
  github.com/git-lfs/git-lfs/v3/lfshttp.(*Client).DoWithRedirect
  \tgithub.com/git-lfs/git-lfs/v3/lfshttp/client.go:335 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).doWithCreds
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:104 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).doWithAuth
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:68 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).DoWithAuth
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:26 (esc)
  github.com/git-lfs/git-lfs/v3/lfsapi.(*Client).DoAPIRequestWithAuth
  \tgithub.com/git-lfs/git-lfs/v3/lfsapi/auth.go:57 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*tqClient).Batch
  \tgithub.com/git-lfs/git-lfs/v3/tq/api.go:92 (esc)
  github.com/git-lfs/git-lfs/v3/tq.Batch
  \tgithub.com/git-lfs/git-lfs/v3/tq/api.go:43 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*TransferQueue).enqueueAndCollectRetriesFor
  \tgithub.com/git-lfs/git-lfs/v3/tq/transfer_queue.go:565 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*TransferQueue).collectBatches.func1
  \tgithub.com/git-lfs/git-lfs/v3/tq/transfer_queue.go:459 (esc)
  runtime.goexit
  \truntime/asm_amd64.s:1594 (esc)
  batch response
  github.com/git-lfs/git-lfs/v3/errors.newWrappedError
  \tgithub.com/git-lfs/git-lfs/v3/errors/types.go:229 (esc)
  github.com/git-lfs/git-lfs/v3/errors.Wrap
  \tgithub.com/git-lfs/git-lfs/v3/errors/errors.go:74 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*tqClient).Batch
  \tgithub.com/git-lfs/git-lfs/v3/tq/api.go:95 (esc)
  github.com/git-lfs/git-lfs/v3/tq.Batch
  \tgithub.com/git-lfs/git-lfs/v3/tq/api.go:43 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*TransferQueue).enqueueAndCollectRetriesFor
  \tgithub.com/git-lfs/git-lfs/v3/tq/transfer_queue.go:565 (esc)
  github.com/git-lfs/git-lfs/v3/tq.(*TransferQueue).collectBatches.func1
  \tgithub.com/git-lfs/git-lfs/v3/tq/transfer_queue.go:459 (esc)
  runtime.goexit
  \truntime/asm_amd64.s:1594 (esc)
  
  Current time in UTC:
  2023-03-12 14:06:11
  
  Environment:
  LocalWorkingDir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo
  LocalGitDir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git
  LocalGitStorageDir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git
  LocalMediaDir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/objects
  LocalReferenceDirs=
  TempDir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/tmp
  ConcurrentTransfers=8
  TusTransfers=false
  BasicTransfersOnly=false
  SkipDownloadErrors=false
  FetchRecentAlways=false
  FetchRecentRefsDays=7
  FetchRecentCommitsDays=0
  FetchRecentRefsIncludeRemotes=true
  PruneOffsetDays=3
  PruneVerifyRemoteAlways=false
  PruneRemoteName=origin
  LfsStorageDir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs
  AccessDownload=none
  AccessUpload=none
  DownloadTransfers=basic,lfs-standalone-file,ssh
  UploadTransfers=basic,lfs-standalone-file,ssh
  GIT_AUTHOR_DATE=2005-04-07T22:13:13
  GIT_AUTHOR_EMAIL=josh@example.com
  GIT_AUTHOR_NAME=Josh
  GIT_COMMITTER_DATE=2005-04-07T22:13:13
  GIT_COMMITTER_EMAIL=josh@example.com
  GIT_COMMITTER_NAME=Josh
  GIT_CONFIG_GLOBAL=/tmp/tmp.GJBLKE
  GIT_CONFIG_NOSYSTEM=1
  GIT_EXEC_PATH=/opt/git-install/libexec/git-core
  
  Client IP addresses:
  172.17.0.2
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.3706017.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.4378166.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.5163076.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.5926727.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.6811169.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.782856.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.8632317.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git-lfs push origin master
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done.
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140611.9388773.log'.
  Use `git lfs logs last` to view the log.
  [2]
  $ git push
  batch response: Fatal error: Server error: http://localhost:8001/real_repo.git/info/lfs/objects/batch
  
  Errors logged to '/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/real_repo/.git/lfs/logs/20230312T140612.0134301.log'.
  Use `git lfs logs last` to view the log.
  error: failed to push some refs to 'http://localhost:8001/real_repo.git'
  [1]

  $ cat ${TESTTMP}/hyper-cgi-test-server.out
  ARGS ["hyper-cgi-test-server", "--port=8001", "--dir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/remote/", "--cmd=git", "--proxy", "/real_repo.git/info/lfs=host.docker.internal:9999", "--args=http-backend"]
  args: ["hyper-cgi-test-server", "--port=8001", "--dir=/tmp/cramtests-jqylvq6j/no_proxy_lfs.t/remote/", "--cmd=git", "--proxy", "/real_repo.git/info/lfs=host.docker.internal:9999", "--args=http-backend"]
  Now listening on 0.0.0.0:8001
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/refs"
  call "/real_repo.git/info/lfs/objects/batch"
  proxy "/objects/batch"
  $ bash ${TESTDIR}/destroy_test_env.sh
  .
  |-- josh
  |   `-- 15
  |       `-- sled
  |           |-- blobs
  |           |-- conf
  |           `-- db
  |-- mirror
  |   |-- HEAD
  |   |-- config
  |   |-- description
  |   |-- info
  |   |   `-- exclude
  |   |-- objects
  |   |   |-- info
  |   |   `-- pack
  |   `-- refs
  |       |-- heads
  |       `-- tags
  `-- overlay
      |-- HEAD
      |-- config
      |-- description
      |-- info
      |   `-- exclude
      |-- objects
      |   |-- info
      |   `-- pack
      `-- refs
          |-- heads
          `-- tags
  
  20 directories, 10 files
