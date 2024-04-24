with import <nixpkgs> {};
mkShell {
  packages = [
    rustup
    pkgsCross.mingwW64.stdenv.cc
    clang
    llvmPackages.libclang # bindgen
  ];
  shellHook = ''
    export CC=x86_64-w64-mingw32-gcc
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-L native=${pkgs.pkgsCross.mingwW64.windows.pthreads}/lib"
    export CARGO_BUILD_TARGET=x86_64-pc-windows-gnu
    export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib" # bindgen
    export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${pkgs.clang}/resource-root/include"
    rustup default stable
    rustup target add x86_64-pc-windows-gnu
  '';
}
