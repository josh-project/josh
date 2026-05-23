# Step 1: Remove `fetch`/`step`/`push` CLI subcommands

## Why

The merge queue runs automatically inside `serve`. The manual `fetch`/`step`/`push`
subcommands were stubs for testing but won't be used. Remove them to simplify the
CLI and avoid dead code.

## What to change

### File: `josh-cq/src/bin/josh-cq.rs`

1. Remove the `ActionCommands` enum entirely (the `#[derive(clap::Subcommand)]` block
   containing `Track`, `Fetch`, `Step`, `Push`).

2. Flatten `Track` directly into the top-level `Commands` enum. After the change,
   `Commands` should have exactly three variants:

```rust
#[derive(clap::Subcommand)]
enum Commands {
    /// Initialize metarepo
    Init,
    /// Start HTTP server
    Serve(ServeArgs),
    /// Track a remote repository
    Track(TrackArgs),
}
```

3. Remove the `Commands::Action(action)` match arm and replace with a direct
   `Commands::Track(args)` arm that calls `handle_track`.

The resulting `main()` match should look like:

```rust
match cli.command {
    Commands::Init => {
        let (_repo_path, _cache, transaction) = open_repo(cli.data_dir.as_deref())?;
        let msg = josh_cq::cq::handle_init(&transaction)?;
        println!("{}", msg);
    }
    Commands::Serve(args) => run_serve(args, cli.data_dir.as_deref()).await?,
    Commands::Track(ref args) => {
        let (_repo_path, _cache, transaction) = open_repo(cli.data_dir.as_deref())?;
        let action = josh_cq::cq::handle_track(&args.url, &args.id, &args.mode, &transaction)?;
        match action {
            josh_cq::cq::UserAction::Message(m) => println!("{m}"),
        }
    }
}
```

4. Check that `TrackArgs` is still reachable (it's referenced by `Commands::Track`).

5. Remove any now-unused imports (e.g., if `ActionCommands` was the only user of
   something).

### Acceptance

- `cargo build --bin josh-cq` succeeds
- `cargo run --bin josh-cq -- --help` shows only `init`, `serve`, `track`
- `cargo fmt` passes
