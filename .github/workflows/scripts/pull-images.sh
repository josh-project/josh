#!/usr/bin/env bash
set -euo pipefail

# Pre-warm podman's local image store from R2 so `josh compose run` skips builds.
# Best-effort: a missing object or transport error just falls through to a local
# build later — never fails the script.
#
# Usage: pull-images.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-images`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "pull-images: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping" >&2
    exit 0
fi

mapfile -t images < <(josh compose list-images --all "$@")

for image in "${images[@]}"; do
    if podman image exists "$image"; then
        echo "pull-images: $image already present locally"
        continue
    fi

    key="images/${image}.tar"
    echo "pull-images: fetching s3://${BUCKET}/${key}"
    if aws s3 cp "s3://${BUCKET}/${key}" - \
            --endpoint-url "$ENDPOINT" \
            --no-progress 2>/dev/null \
        | podman load; then
        echo "pull-images: loaded $image"
    else
        echo "pull-images: $image not in R2 (will build locally)"
    fi
done
