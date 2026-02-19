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
  
  # Python with packages needed for prysk
  pythonWithPackages = pkgs.python3.withPackages (ps: with ps; [
    pip
    setuptools
    wheel
  ]);
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
      pythonWithPackages
    ] ++ extra_deps;
    
    RUST_BACKTRACE = 1;
    
    shellHook = ''
      echo "Welcome to Josh development environment!"
      echo "Rust version: $(rustc --version)"
      echo "Cargo version: $(cargo --version)"
      echo "Python version: $(python3 --version)"
      
      # Install prysk using pip in user space with specific version
      if ! command -v prysk &> /dev/null; then
        echo "Installing prysk..."
        pip install --user "prysk==0.20.0"
      else
        # Ensure we have the correct version
        pip install --user --upgrade "prysk==0.20.0"
      fi
      
      echo "Prysk version: $(prysk --version 2>/dev/null || echo 'Installing...')"
      echo "Environment ready! Run tests with: ./run-tests.sh"
    '';
  }
