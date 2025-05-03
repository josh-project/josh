# Testing
Currently the Josh project mainly uses integration tests for it's verification, so make sure you will be able
to run and check them.

The following sections will describe how to run the different kind's of tests used for the verification
of the Josh project.

## UnitTests & DocTests
```shell
cargo test --all
```

## Integration Tests

### 1. Setup the test environment
Due to the fact that the integration tests need additional tools and a more complex
environment and due to the fact that the integration test are done using [cram](https://pypi.org/project/cram/).
you will need to crate an extra environment to run these tests. To simplify the
setup of the integration testing we have set up a [Nix Shell](https://nixos.org/manual/nix/stable/#chap-installation) environment which
you can start by using the following command if you have installed the [Nix Shell](https://nixos.org/manual/nix/stable/#chap-installation).

**Attention:**
Currently it is still necessary to install the following tools in your host system.
* curl
* hyper_cgi
    ```shell
    cargo install hyper_cgi --features=test-server
    ```

#### Setup the Nix Shell
**Attention:**
When running this command the first time, this command will take quite a bit to
finish. You also will need internet access while executing this command.
Depending on performance of your connection the command will take more or less time.

```shell
nix-shell shell.nix
```
Once the command is finished you will be prompted with the nix-shell which will
provide the needed shell environment to execute the integration tests.


### 2. Verify you have built all necessary binaries
```shell
cargo build
cargo build --bin josh-filter
cargo build --manifest-path josh-proxy/Cargo.toml
cargo build --manifest-path josh-ui/Cargo.toml
```

### 3. Setup static files for the josh-ui
```shell
cd josh-ui 
trunk build 
cd ..
```

### 4. Run the integration tests
**Attention:** Be aware that all tests except the once in experimental should be green.
```shell
sh run-tests.sh -v tests/
```

## UI Tests
TBD: Currently disabled, stabilize, enable and document process.