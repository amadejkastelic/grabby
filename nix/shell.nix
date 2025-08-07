{
  pkgs ? import <nixpkgs-unstable> { },
  pre-commit-check ? null,
}:
pkgs.mkShell {
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
  inputsFrom = [ (pkgs.callPackage ./default.nix { }) ];
  buildInputs = with pkgs; [
    rust-analyzer
    rustfmt
    clippy
    yt-dlp
  ];
  shellHook = if pre-commit-check != null then pre-commit-check.shellHook else "";
}
