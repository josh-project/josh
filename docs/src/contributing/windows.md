# Windows

Windows support is experimental. `josh-proxy` and the `josh` / `josh-filter` CLIs build and run;
SSH serving and `josh compose` do not (see [Limitations](#limitations)).

## Setup

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Git.Git
```

Git for Windows is needed at runtime, not just to clone: josh shells out to `git`, and the hooks
it installs are `sh` shims that git runs with the bundled sh.

On ARM64, also install LLVM and build with clang-cl: the `aws-lc-sys` dependency has GNU-syntax
ARM assembly that MSVC cannot assemble, and the build otherwise fails late in `lib.exe` with
LNK1181, archiving object files that were never written.

```powershell
winget install LLVM.LLVM
$env:Path += ';C:\Program Files\LLVM\bin'; $env:CC='clang-cl'; $env:CXX='clang-cl'
```

Pass the bare name via `PATH` rather than a full path in `CC`: cc-rs splits that variable on
whitespace.

## Build

`josh-ssh-shell` is unix-only, so build the supported binaries rather than the workspace:

```powershell
cargo build --release -p josh-proxy -p josh-cli
```

## Limitations

* SSH is not supported on Windows.
* `josh compose run` is not supported on Windows, so the repository's own test suite does not
  run there.
* An upstream whose path contains a reserved Windows device name (`aux`, `con`, `nul`,
  `com1`..`com9`, `lpt1`..`lpt9`) cannot be mirrored: the namespace becomes a path, and Windows
  has no such filename.
* Windows support is experimental.
