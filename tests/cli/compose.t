  $ export TESTTMP=${PWD}
  $ export JOSH_EXPERIMENTAL_FEATURES=1

Create a remote compose result ref.

  $ git init -q --bare remote
  $ git init -q seed
  $ cd seed
  $ echo result > result
  $ git add result
  $ git commit -q -m "seed compose result"
  $ git push -q ${TESTTMP}/remote HEAD:refs/josh/compose
  $ remote_oid=$(git rev-parse HEAD)

Pulling compose metadata must persist the ref for later CLI invocations.

  $ git init -q ${TESTTMP}/local
  $ cd ${TESTTMP}/local
  $ git for-each-ref --format='%(refname)' refs/josh/compose
  $ josh compose pull --remote ${TESTTMP}/remote 2>/dev/null
  $ test "$(git rev-parse refs/josh/compose)" = "${remote_oid}"
  $ git cat-file -t refs/josh/compose
  commit

Compose graphing must discard objects created while applying the workspace filter.

  $ git init -q ${TESTTMP}/filtered
  $ cd ${TESTTMP}/filtered
  $ echo ':$label="ephemeral-workspace"' > compose.josh
  $ git add compose.josh
  $ git commit -q -m "add compose workspace"
  $ workspace=$(josh compose list-jobs --all HEAD)

Abbreviated commit SHAs must select the same compose input.

  $ short=$(git rev-parse --short HEAD)
  $ test "$(josh compose list-jobs --all "${short}")" = "${workspace}"
  $ josh compose graph HEAD | sed -E 's/[0-9a-f]{40}/OID/g'
  direction: down
  job_OID: "ephemeral-workspace"
  $ test -n "${workspace}"
  $ git cat-file -e "${workspace}" 2>/dev/null
  [1]
