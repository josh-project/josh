#!/bin/sh
set -eu

# Extract hostname from the full endpoint URL.
# e.g. https://xxx.r2.cloudflarestorage.com -> xxx.r2.cloudflarestorage.com
host=$(echo "$SCCACHE_ENDPOINT" | awk -F/ '{print $3}')

exec aws-sigv4-proxy \
    --name s3 \
    --region "${SCCACHE_REGION:-auto}" \
    --host "$host"
