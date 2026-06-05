#!/usr/bin/env bash
set -euo pipefail

# Pre-warm .josh/success markers and out_<hash> volumes from R2 so
# `josh compose run` cache-skips workspaces whose results already exist.
# Best-effort: a missing object or transport error just falls through and
# the corresponding job runs locally.
#
# Two passes:
#   1. Pull every success marker for the universe of jobs the run would touch.
#   2. After markers land, `list-jobs` (cache-aware) returns hashes that still
#      aren't fully cached locally. For those, pull the output volume tarball.
#
# Usage: pull-jobs.sh [REFERENCE] [FILTER]
# Extra args are forwarded to `josh compose list-jobs`.

BUCKET="josh-project-cache"
ENDPOINT="https://19f2dfdd7c93980184be5e5809e8b252.r2.cloudflarestorage.com"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    echo "pull-jobs: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set, skipping" >&2
    exit 0
fi

mkdir -p .josh/success

# Pass 1: pull every success marker for the universe of jobs.
mapfile -t universe < <(josh compose list-jobs --all "$@")

for hash in "${universe[@]}"; do
    if [[ -f ".josh/success/${hash}" ]]; then
        echo "pull-jobs: marker $hash already present locally"
        continue
    fi

    key="job-markers/${hash}"
    if aws s3 cp "s3://${BUCKET}/${key}" ".josh/success/${hash}" \
            --endpoint-url "$ENDPOINT" \
            --no-progress 2>/dev/null; then
        echo "pull-jobs: marker $hash"
    else
        echo "pull-jobs: marker $hash not in R2"
    fi
done

# Pass 2: cache-aware list returns hashes still missing volume (or marker).
# For each, try to pull the volume tarball into a fresh podman volume.
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
        # Failed pull -> drop the volume if we partially created it, and drop
        # the marker so the job re-runs cleanly. The Rust skip-check tightening
        # would self-heal this anyway, but cleaning up here keeps state tidy.
        if podman volume exists "$vol"; then
            podman volume rm --force "$vol" >/dev/null
        fi
        rm -f ".josh/success/${hash}"
        echo "pull-jobs: volume $hash not in R2 (will build locally)"
    fi
done
