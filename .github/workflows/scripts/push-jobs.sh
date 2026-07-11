#!/usr/bin/env bash
set -euo pipefail

# Upload .josh/success markers and out_<hash> output volumes to R2, so future
# runs can pull instead of rebuild. Idempotent: existing objects are detected
# via head-object and skipped. Failed (.josh/failed) markers are not uploaded.
#
# Usage: push-jobs.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-jobs`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "push-jobs: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping" >&2
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
    # Marker
    if [[ -f ".josh/success/${hash}" ]]; then
        key="job-markers/${hash}"
        if head_exists "$key"; then
            echo "push-jobs: marker $hash already in R2"
        else
            echo "push-jobs: uploading marker $hash -> s3://${BUCKET}/${key}"
            aws s3 cp ".josh/success/${hash}" "s3://${BUCKET}/${key}" \
                --endpoint-url "$ENDPOINT" \
                --no-progress
        fi
    else
        echo "push-jobs: marker $hash not present locally, skipping"
    fi

    # Volume (absent for output=none workspaces — naturally skipped)
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
