# Dev Tools

## Nix-Shell
* [Nix Shell](https://nixos.org/manual/nix/stable/#chap-installation)


# Testing

1. Install Nix-Shell
2. Setup Nix-Shell for testing
```shell
nix-shell shell.nix
```
3. build 
   * filter
   * proxy
   * trunk build in ui


## Unit Tests
```shell
cargo test --all
```

## Integration Tests
* Build all targets
* run integration tests
```shell
sh run-tests.sh tests/
```

## UI Tests
TBD