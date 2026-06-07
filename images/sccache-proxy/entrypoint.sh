#!/bin/sh
set -eu

# Extract hostname from the full upstream endpoint URL.
# e.g. https://xxx.r2.cloudflarestorage.com -> xxx.r2.cloudflarestorage.com
host=$(echo "$UPSTREAM_ENDPOINT" | awk -F/ '{print $3}')

exec aws-sigv4-proxy \
    --name s3 \
    --region "auto" \
    --port ":${PORT}" \
    --host "$host"
