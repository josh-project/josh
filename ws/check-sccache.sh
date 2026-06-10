set -eu

stats=$(sccache --show-stats --stats-format json)
echo "$stats" | jq .

# sccache silently falls back to the local disk cache when the configured remote
# backend can't be reached. On dev machines without R2 credentials this is expected,
# so emit a grepable warning instead of failing — CI greps for the marker and turns
# it back into a hard failure. The S3-backed cache reports a cache_location like
# "S3, bucket: ...".
if echo "$stats" | jq -e '.cache_location | ascii_downcase | startswith("s3")' >/dev/null
then
    echo "sccache: confirmed S3-backed cache"
else
    echo "SCCACHE_REMOTE_CACHE_MISSING: sccache is not using the S3 remote cache" >&2
    echo "cache_location: $(echo "$stats" | jq -r '.cache_location')" >&2
    env | grep -E '^(SCCACHE|RUSTC_WRAPPER|CARGO)' | sort >&2
fi
