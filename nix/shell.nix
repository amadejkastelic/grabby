{
  pkgs ? import <nixos-unstable> { },
  rustPlatform ? pkgs.rustPlatform,
  devToolchain ? pkgs.rust.packages.stable.rustPlatform.rustc,
  pre-commit-check ? null,
}:
pkgs.mkShell {
  RUST_SRC_PATH = "${devToolchain}/lib/rustlib/src/rust/library";
  inputsFrom = [ (pkgs.callPackage ./package.nix { inherit rustPlatform; }) ];
  buildInputs = [
    devToolchain
    pkgs.rust-analyzer
    pkgs.pkg-config
    pkgs.openssl
    pkgs.cargo-edit
    pkgs.yt-dlp
    pkgs.ffmpeg
    pkgs.gallery-dl
  ];
  shellHook = if pre-commit-check != null then pre-commit-check.shellHook else "";
}
