#!/usr/bin/env bash
set -euo pipefail

# Pull compose results through `josh compose pull`, then pre-warm their
# out_<hash> volumes from R2. R2 stores only output volume tarballs.
#
# Usage: pull-jobs.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-jobs`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"
CACHE_REMOTE="${JOSH_COMPOSE_REMOTE:-origin}"

josh compose pull --remote "$CACHE_REMOTE"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "pull-jobs: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping volumes" >&2
    exit 0
fi

mapfile -t need < <(josh compose list-jobs "$@")

for hash in "${need[@]}"; do
    vol="out_${hash}"
    if podman volume exists "$vol"; then
        echo "pull-jobs: volume $hash already present locally"
        continue
    fi

    key="job-volumes/${hash}.tar"
    if aws s3 cp "s3://${BUCKET}/${key}" - \
            --endpoint-url "$ENDPOINT" \
            --no-progress 2>/dev/null \
        | { podman volume create "$vol" >/dev/null \
            && podman volume import "$vol" -; }; then
        echo "pull-jobs: volume $hash"
    else
        if podman volume exists "$vol"; then
            podman volume rm --force "$vol" >/dev/null
        fi
        echo "pull-jobs: volume $hash not in R2 (will build locally)"
    fi
done
