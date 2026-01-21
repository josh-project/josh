  $ git init --bare -q repo.git
  $ cd repo.git

  $ TREE=$(git write-tree)
  $ COMMIT=$(git commit-tree $TREE -m "Initial commit")
  $ git update-ref refs/heads/main $COMMIT

  $ josh-mq init

  $ josh-mq config remote add origin https://example.com/repo.git --credential token
  Added remote 'origin'

  $ git show main:.mq.toml
  [remotes.origin]
  url = "https://example.com/repo.git"
  main = "main"
  credential = "token"

  $ josh-mq config remote remove origin
  Removed remote 'origin'

  $ git log --oneline main
  0411248 Remove remote 'origin'
  8c4c246 Add remote 'origin'
  e610309 Init metarepo
  2f464fe Initial commit
