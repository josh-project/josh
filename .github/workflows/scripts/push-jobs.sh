#!/usr/bin/env bash
set -euo pipefail

# Push compose results through `josh compose push`, then upload out_<hash>
# output volumes to R2. R2 stores only output volume tarballs.
#
# Usage: push-jobs.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-jobs`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"
CACHE_REMOTE="${JOSH_COMPOSE_REMOTE:-origin}"

josh compose push --remote "$CACHE_REMOTE"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "push-jobs: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping volumes" >&2
    exit 0
fi

head_exists() {
    aws s3api head-object \
        --bucket "$BUCKET" \
        --key "$1" \
        --endpoint-url "$ENDPOINT" \
        >/dev/null 2>&1
}

mapfile -t hashes < <(josh compose list-jobs --all "$@")

for hash in "${hashes[@]}"; do
    vol="out_${hash}"
    if podman volume exists "$vol"; then
        key="job-volumes/${hash}.tar"
        if head_exists "$key"; then
            echo "push-jobs: volume $hash already in R2"
        else
            echo "push-jobs: uploading volume $hash -> s3://${BUCKET}/${key}"
            podman volume export "$vol" \
                | aws s3 cp - "s3://${BUCKET}/${key}" \
                    --endpoint-url "$ENDPOINT" \
                    --no-progress
        fi
    fi
done
