{
  pkgs ? import <nixos-unstable> { },
  pre-commit-check ? null,
}:
pkgs.mkShell {
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
  inputsFrom = [ (pkgs.callPackage ./package.nix { }) ];
  buildInputs = with pkgs; [
    rust-analyzer
    rustfmt
    clippy
    pkg-config
    openssl

    yt-dlp
    ffmpeg
    gallery-dl
  ];
  shellHook = if pre-commit-check != null then pre-commit-check.shellHook else "";
}
