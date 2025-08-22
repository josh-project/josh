let
  pkgs = import (fetchTarball {
    name = "nixos-25.05";
    url = "https://github.com/NixOS/nixpkgs/archive/refs/tags/25.05.tar.gz";
    sha256 = "1915r28xc4znrh2vf4rrjnxldw2imysz819gzhk9qlrkqanmfsxd";
  }) {};
  
  pythonWithPryskDeps = pkgs.python3.withPackages (ps: with ps; [
    pip
    setuptools
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
       pythonWithPryskDeps
     ];
     RUST_BACKTRACE = 1;
     
     shellHook = ''
       echo "Welcome to Josh development environment!"
       echo "Rust version: $(rustc --version)"
       echo "Cargo version: $(cargo --version)"
       
       # Install prysk using pip in a virtual environment
       if [ ! -d ".venv" ]; then
         echo "Creating Python virtual environment..."
         python3 -m venv .venv
       fi
       
       source .venv/bin/activate
       
       # Install or upgrade prysk
       echo "Installing/updating prysk..."
       pip install --upgrade pip
       pip install --upgrade prysk
       
       echo "Prysk installed: $(prysk --version 2>/dev/null || echo 'Run prysk --help for usage')"
     '';
   }
