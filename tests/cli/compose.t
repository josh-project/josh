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
  $ mkdir -p image-context images
  $ echo 'FROM scratch' > image-context/Dockerfile
  $ echo ':$label="friendly image"' > images/test.josh
  $ echo 'context = :/image-context' >> images/test.josh
  $ echo ':$label="ephemeral-workspace"' > compose.josh
  $ echo ':#image[:+images/test]' >> compose.josh
  $ echo ':$output="none"' >> compose.josh
  $ echo 'worktree = :/image-context' >> compose.josh
  $ git add compose.josh image-context images
  $ git commit -q -m "add compose workspace"
  $ workspace=$(josh compose list-jobs --all HEAD)

Abbreviated commit SHAs must select the same compose input.

  $ short=$(git rev-parse --short HEAD)
  $ test "$(josh compose list-jobs --all "${short}")" = "${workspace}"
  $ josh compose graph HEAD | sed -E 's/[0-9a-f]{40}/OID/g'
  direction: down
  image_OID: "image friendly image"
  job_OID: "ephemeral-workspace"
  image_OID -> job_OID: "image"
  $ test -n "${workspace}"
  $ git cat-file -e "${workspace}" 2>/dev/null
  [1]

Compose run status lines use the image label rather than its content hash.

  $ mkdir bin
  $ printf '%s\n' '#!/bin/sh' 'if [ "$1" = image ]; then exit 1; fi' 'if [ "$1" = build ]; then cat >/dev/null; fi' 'exit 0' > bin/docker
  $ chmod +x bin/docker
  $ PATH="${PWD}/bin:${PATH}" josh compose run --backend docker HEAD 2>&1 | sed -E 's/[0-9a-f]{40}/OID/g'
  [ephemeral-workspace] Running (OID)
  [image:friendly image] Building...
  [image:friendly image] Built successfully
  [ephemeral-workspace] SUCCESS

Revision object expressions resolve across every compose planning command.

  $ git init -q ${TESTTMP}/revisions
  $ cd ${TESTTMP}/revisions
  $ mkdir -p ws
  $ cat > ws/context.josh <<'EOF'
  > :$label="revision job"
  > :$output="none"
  > worktree = :[
  >     :$.={#baseline|#^}
  > ]
  > EOF
  $ echo parent > selected
  $ git add .
  $ git commit -q -m "parent"
  $ parent=$(git rev-parse HEAD)
  $ echo current > selected
  $ git commit -q -am "current"
  $ current=$(git rev-parse HEAD)
  $ git checkout -q -b explicit "${parent}"
  $ echo explicit > selected
  $ git commit -q -am "explicit"
  $ explicit=$(git rev-parse HEAD)
  $ git checkout -q master
  $ filter=:+ws/context
  $ default_job=$(josh compose list-jobs --all "${current}" "${filter}")
  $ explicit_job=$(josh compose list-jobs --all --arg baseline="${explicit}" "${current}" "${filter}")
  $ test -n "${default_job}"
  $ test -n "${explicit_job}"
  $ test "${default_job}" != "${explicit_job}"
  $ default_graph=$(josh compose graph "${current}" "${filter}")
  $ explicit_graph=$(josh compose graph --arg baseline="${explicit}" "${current}" "${filter}")
  $ case "${default_graph}" in *"job_${default_job}"*) true;; *) false;; esac
  $ case "${explicit_graph}" in *"job_${explicit_job}"*) true;; *) false;; esac
  $ test -z "$(josh compose list-images --all "${current}" "${filter}")"
  $ test -z "$(josh compose list-images --all --arg baseline="${explicit}" "${current}" "${filter}")"
  $ mkdir bin
  $ printf '%s\n' '#!/bin/sh' 'if [ "$1" = image ]; then exit 1; fi' 'if [ "$1" = build ]; then cat >/dev/null; fi' 'exit 0' > bin/docker
  $ chmod +x bin/docker
  $ PATH="${PWD}/bin:${PATH}" josh compose run --backend docker "${current}" "${filter}" >/dev/null
  [revision job] Running (aa92c051291e109700d69ab90869ccadfd476f6c)
  [revision job] Done (orchestrator)

  $ PATH="${PWD}/bin:${PATH}" josh compose run --backend docker --arg baseline="${explicit}" "${current}" "${filter}" >/dev/null
  [revision job] Running (972881bf274d3c5b76e9db11bb692ef996b88299)
  [revision job] Done (orchestrator)

the synthetic snapshot commit exists only in the transaction object store.

  $ echo working > selected
  $ working_default=$(josh compose list-jobs --all . "${filter}")
  $ working_explicit=$(josh compose list-jobs --all --arg baseline=HEAD . "${filter}")
  $ test "${working_default}" = "${working_explicit}"
  $ git add selected
  $ index_default=$(josh compose list-jobs --all + "${filter}")
  $ index_explicit=$(josh compose list-jobs --all --arg baseline=HEAD + "${filter}")
  $ test "${index_default}" = "${index_explicit}"
  $ git reset -q --hard

the parent fallback.

  $ git merge -q --no-ff -s ours explicit -m "merge"
  $ merge=$(git rev-parse HEAD)
  $ merge_default=$(josh compose list-jobs --all "${merge}" "${filter}")
  $ merge_explicit=$(josh compose list-jobs --all --arg baseline="${current}" "${merge}" "${filter}")
  $ test "${merge_default}" = "${merge_explicit}"
  $ root=$(printf 'root\n' | git commit-tree "${parent}^{tree}")
  $ josh compose list-jobs --all "${root}" "${filter}" >/dev/null 2>&1
  [1]

  $ josh compose list-jobs --all --arg baseline="${explicit}" "${root}" "${filter}" >/dev/null

workspace identity between invocations.

  $ git branch -f moving "${parent}"
  $ moving_parent=$(josh compose list-jobs --all --arg baseline=moving "${merge}" "${filter}")
  $ git branch -f moving "${explicit}"
  $ moving_explicit=$(josh compose list-jobs --all --arg baseline=moving "${merge}" "${filter}")
  $ test "${moving_parent}" != "${moving_explicit}"
  $ josh compose list-jobs --all --arg baseline=missing "${merge}" "${filter}" >/dev/null 2>&1
  [1]

  $ josh compose list-jobs --all --arg baseline="${parent}..${merge}" "${merge}" "${filter}" >/dev/null 2>&1
  [1]

  $ josh compose list-jobs --all --arg baseline="${parent}" --arg baseline="${explicit}" "${merge}" "${filter}" >/dev/null 2>&1
  [1]

  $ josh compose list-jobs --all --arg input=HEAD "${merge}" "${filter}" >/dev/null 2>&1
  [2]
