{
  description = "Grabby - Media Embedding Bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    pre-commit-hooks = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      pre-commit-hooks,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        grabby = pkgs.callPackage ./nix/default.nix { };

        pre-commit-check = pre-commit-hooks.lib.${system}.run {
          src = ./.;
          hooks = {
            nixfmt-rfc-style.enable = true;
            cargo-check.enable = true;
            rustfmt.enable = true;
            clippy = {
              enable = true;
              settings.denyWarnings = true;
            };
          };
        };
      in
      {
        packages.default = grabby;

        devShells.default = pkgs.callPackage ./nix/shell.nix {
          inherit pre-commit-check;
        };

        checks = {
          pre-commit-check = pre-commit-check;
          package = grabby;
        };
      }
    );
}
