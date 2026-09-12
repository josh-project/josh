#!/usr/bin/env bash
set -euo pipefail

# Provide the CI bootstrap `josh` binary at target/release/josh, used by the
# workflow to drive `josh compose run`, sync its Git result ref, and transfer
# cached volumes and images through R2.
#
# Strategy:
#   1. If R2 credentials are present, try to download a binary built from the
#      pinned `JOSH_BOOTSTRAP_SHA` commit. Hit means seconds; miss falls
#      through to build.
#   2. On miss (or no credentials), check out the pinned SHA into a separate
#      worktree, build `josh-cli`, and place the binary at target/release/josh.
#   3. If R2 credentials are present after a build, upload so the next run on
#      any branch finds the object.
#
# The pin must provide the compose metadata and transport implementation used
# by this workflow, including `josh compose pull` and `josh compose push`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"

if [[ -z "${JOSH_BOOTSTRAP_SHA:-}" ]]; then
    echo "bootstrap-josh: JOSH_BOOTSTRAP_SHA is not set" >&2
    exit 1
fi

KEY="bootstrap/josh-${JOSH_BOOTSTRAP_SHA}-linux-amd64"
DEST="target/release/josh"
mkdir -p target/release

have_creds=0
if [[ -n "${AWS_ACCESS_KEY_ID:-}" && -n "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    have_creds=1
fi

if [[ "$have_creds" == "1" ]]; then
    if aws s3 cp "s3://${BUCKET}/${KEY}" "${DEST}" \
            --endpoint-url "$ENDPOINT" \
            --no-progress 2>/dev/null; then
        chmod +x "${DEST}"
        echo "bootstrap-josh: downloaded ${KEY} from R2"
        exit 0
    fi
    echo "bootstrap-josh: ${KEY} not in R2, building from pinned ref"
else
    echo "bootstrap-josh: no R2 credentials, building from pinned ref"
fi

WORKTREE="$(mktemp -d -t josh-bootstrap-XXXXXX)"
trap 'git worktree remove --force "$WORKTREE" >/dev/null 2>&1 || true; rm -rf "$WORKTREE"' EXIT

# Ensure the pinned SHA is present locally. The CI checkout may not contain it
# (for example, when a pull request checkout only fetches its head).
if ! git cat-file -e "${JOSH_BOOTSTRAP_SHA}^{commit}" 2>/dev/null; then
    git fetch --no-tags --depth=1 origin "$JOSH_BOOTSTRAP_SHA"
fi

git worktree add --detach "$WORKTREE" "$JOSH_BOOTSTRAP_SHA"
(cd "$WORKTREE" && cargo build --release -p josh-cli)
cp "${WORKTREE}/target/release/josh" "${DEST}"
chmod +x "${DEST}"

if [[ "$have_creds" == "1" ]]; then
    if aws s3 cp "${DEST}" "s3://${BUCKET}/${KEY}" \
            --endpoint-url "$ENDPOINT" \
            --no-progress; then
        echo "bootstrap-josh: uploaded ${KEY} to R2"
    else
        echo "bootstrap-josh: upload to R2 failed (non-fatal)"
    fi
fi
