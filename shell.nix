with import <nixpkgs> {};
mkShell {
  packages = [
    rustup
    alsa-lib
    pkg-config
    clang
    llvmPackages.libclang # bindgen

    # --- nih-plug with egui ---
    jack2
    python3
    xorg.xcbutilwm xcbuild xcb-util-cursor xcbutilxrm # something-something xcb-icccm
    libxkbcommon
    libGL
    wayland
    xorg.libXcursor
    xorg.libXrandr
    xorg.libXi
    xorg.libX11
  ];
  shellHook = ''
    unset CC
    unset CARGO_BUILD_TARGET
    export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib" # bindgen
    export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${pkgs.clang}/resource-root/include"
    rustup default stable
  '';
}
