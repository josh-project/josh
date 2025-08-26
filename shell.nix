let
  pkgs = import (fetchTarball {
    name = "nixos-24.11";
    url = "https://github.com/NixOS/nixpkgs/archive/refs/tags/24.11.tar.gz";
    # Hash obtained using `nix-prefetch-url --unpack <url>`
    sha256 = "1250a3g9g4id46h9ysy5xfqnjf0yb2mfai366pyj5y2bzb8x0i2l";
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