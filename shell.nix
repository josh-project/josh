let
  pkgs = import (fetchTarball {
    name = "nixpkgs-release-25.11";
    url = "https://github.com/NixOS/nixpkgs/archive/871b9fd269ff6246794583ce4ee1031e1da71895.tar.gz";
    # Hash obtained using `nix-prefetch-url --unpack <url>`
    sha256 = "1zn1lsafn62sz6azx6j735fh4vwwghj8cc9x91g5sx2nrg23ap9k";
  }) {};
  
  # Handle darwin-specific dependencies properly
  extra_deps = if pkgs.stdenv.isDarwin then [
    pkgs.darwin.apple_sdk.frameworks.Security
    pkgs.darwin.apple_sdk.frameworks.CoreFoundation
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ] else [];
  
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
      pkgs.pkg-config
      pkgs.nodejs
    ] ++ extra_deps;
    
    RUST_BACKTRACE = 1;
    
    shellHook = ''
      echo "Welcome to Josh development environment!"
      echo "Rust version: $(rustc --version)"
      echo "Cargo version: $(cargo --version)"

      # Install scrut using cargo with specific version
      if ! command -v scrut &> /dev/null; then
        echo "Installing scrut..."
        cargo install --version 0.4.3 scrut
      fi

      echo "Scrut version: $(scrut --version 2>/dev/null || echo 'Installing...')"
      echo "Environment ready! Run tests with: ./run-tests.sh"
    '';
  }
