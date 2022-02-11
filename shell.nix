with (import <nixpkgs> {});

let
  extra_deps = if stdenv.isDarwin then [
    darwin.apple_sdk.frameworks.Security
  ] else [];
  pkgs = import ( fetchTarball {
      name = "nixos-21.11";
      url =  "https://github.com/NixOS/nixpkgs/archive/refs/tags/21.11.tar.gz";
      # Hash obtained using `nix-prefetch-url --unpack <url>`
      sha256 = "162dywda2dvfj1248afxc45kcrg83appjd0nmdb541hl7rnncf02";
  }) {};
  rust_channel = nixpkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain;
in
   pkgs.mkShell {
     buildInputs = [
       pkgs.git
       pkgs.tree
       pkgs.cargo
       pkgs.rustc
       pkgs.trunk
       pkgs.rustfmt
       pkgs.libiconv
       pkgs.openssl.dev
       pkgs.pkgconfig
       pkgs.python39Packages.cram
     ] ++ extra_deps;
     RUST_BACKTRACE = 1;
   }

