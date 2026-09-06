#!/usr/bin/env bash
set -euo pipefail

# Upload josh_out_<hash> output volumes to R2. Compose result metadata is
# published separately, from a job with repository write permission.
#
# Usage: push-volumes.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-jobs`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "push-volumes: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping" >&2
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
    vol="josh_out_${hash}"
    if podman volume exists "$vol"; then
        key="job-volumes/${hash}.tar"
        if head_exists "$key"; then
            echo "push-volumes: volume $hash already in R2"
        else
            echo "push-volumes: uploading volume $hash -> s3://${BUCKET}/${key}"
            podman volume export "$vol" \
                | aws s3 cp - "s3://${BUCKET}/${key}" \
                    --endpoint-url "$ENDPOINT" \
                    --no-progress
        fi
    fi
done
