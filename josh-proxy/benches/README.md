# Benchmarks

## Requirements

- [samply](https://github.com/mstange/samply)
- [cargo-samply](https://crates.io/crates/cargo-samply) for convenient cargo integration

## Running

First, build josh-proxy in profiling configuration:

```bash
cargo build -p josh-proxy --profile profiling
```

Then run the benchmark with samply:

```bash
cargo samply -p josh-proxy --bench push_upstream --profile profiling -- \
  --upstream-dir /path/to/upstream \
  --proxy-dir /path/to/data \
  --local-dir /path/to/local \
  --source-ref refs/heads/main \
  --josh-proxy-path $(readlink -f target/profiling/josh-proxy)
```

Replace the directory paths with your actual benchmark repository locations.
