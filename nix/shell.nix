{
  pkgs ? import <nixos-unstable> { },
  rustPlatform ? pkgs.rustPlatform,
  devToolchain ? pkgs.rust.packages.stable.rustPlatform.rustc,
  pre-commit-check ? null,
}:
pkgs.mkShell {
  RUST_SRC_PATH = "${devToolchain}/lib/rustlib/src/rust/library";
  inputsFrom = [ (pkgs.callPackage ./package.nix { inherit rustPlatform; }) ];
  buildInputs =
    with pkgs;
    [
      rust-analyzer
      pkg-config
      openssl
      cargo-edit
      yt-dlp
      ffmpeg
      gallery-dl
    ]
    ++ [ devToolchain ];
  shellHook = if pre-commit-check != null then pre-commit-check.shellHook else "";
}
