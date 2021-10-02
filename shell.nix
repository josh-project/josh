with (import <nixpkgs> {});

let
  extra_deps = if stdenv.isDarwin then [
    darwin.apple_sdk.frameworks.Security
  ] else [];
  pkgs = import ( fetchTarball {
      name = "nixos-21.05";
      url =  "https://github.com/NixOS/nixpkgs/archive/refs/tags/21.05.tar.gz";
      # Hash obtained using `nix-prefetch-url --unpack <url>`
      sha256 = "1ckzhh24mgz6jd1xhfgx0i9mijk6xjqxwsshnvq789xsavrmsc36";
  }) {};
  rust_channel = nixpkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain;
in
   pkgs.mkShell {
     buildInputs = [
       pkgs.git
       pkgs.tree
       pkgs.cargo
       pkgs.rustc
       pkgs.rustfmt
       pkgs.libiconv
       pkgs.openssl.dev
       pkgs.pkgconfig
       pkgs.python39Packages.cram
     ] ++ extra_deps;
     RUST_BACKTRACE = 1;
   }
