{
  pkgs ? import <nixpkgs> { },
}:
let
  manifest = (pkgs.lib.importTOML ../Cargo.toml).package;
in
pkgs.rustPlatform.buildRustPackage {
  pname = manifest.name;
  version = manifest.version;
  cargoLock.lockFile = ../Cargo.lock;
  src = pkgs.lib.cleanSource ../.;
  doCheck = true;

  nativeBuildInputs = [ pkgs.makeWrapper ];

  buildInputs = with pkgs; [
    yt-dlp
    gallery-dl
    ffmpeg
  ];

  postFixup = ''
    wrapProgram $out/bin/grabby \
      --prefix PATH : "${
        pkgs.lib.makeBinPath [
          pkgs.yt-dlp
          pkgs.gallery-dl
          pkgs.ffmpeg
        ]
      }"
  '';
}
