#!/usr/bin/env bash
set -euo pipefail

# Upload any podman images a run needed to R2, so future runs can pull instead of
# build. Idempotent: existing objects are detected via head-object and skipped.
#
# Usage: push-images.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-images`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "push-images: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping" >&2
    exit 0
fi

mapfile -t images < <(josh compose list-images --all "$@")

for image in "${images[@]}"; do
    if ! podman image exists "$image"; then
        echo "push-images: $image not present locally, skipping"
        continue
    fi

    key="images/${image}.tar"
    if aws s3api head-object \
            --bucket "$BUCKET" \
            --key "$key" \
            --endpoint-url "$ENDPOINT" \
            >/dev/null 2>&1; then
        echo "push-images: s3://${BUCKET}/${key} already present, skipping"
        continue
    fi

    echo "push-images: uploading $image -> s3://${BUCKET}/${key}"
    podman save --format=docker-archive "$image" \
        | aws s3 cp - "s3://${BUCKET}/${key}" \
            --endpoint-url "$ENDPOINT" \
            --no-progress
done
