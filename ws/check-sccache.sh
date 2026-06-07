set -eu

stats=$(sccache --show-stats --stats-format json)
echo "$stats" | jq .

# sccache silently falls back to the local disk cache when the configured remote
# backend can't be reached. Fail loudly so a broken R2/proxy setup doesn't pass as a
# green build. The S3-backed cache reports a cache_location like "S3, bucket: ...".
if echo "$stats" | jq -e '.cache_location | ascii_downcase | startswith("s3")' >/dev/null
then
    echo "sccache: confirmed S3-backed cache"
else
    echo "ERROR: sccache is not using the S3 remote cache" >&2
    echo "cache_location: $(echo "$stats" | jq -r '.cache_location')" >&2
    env | grep -E '^(SCCACHE|RUSTC_WRAPPER|CARGO)' | sort >&2
    exit 1
fi
