with (import <nixpkgs> {});

let
  extra_deps = if stdenv.isDarwin then [
    darwin.apple_sdk.frameworks.Security
  ] else [];

in
   mkShell {
     buildInputs = [
       git
       tree
       cargo
       libiconv
       openssl.dev
       pkgconfig
       python39Packages.cram
     ] ++ extra_deps;
   }
