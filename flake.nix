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

        grabbyPkg = pkgs.callPackage ./nix/default.nix { };

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
        packages = rec {
          grabby = grabbyPkg;

          default = grabby;

          docker = import ./nix/docker.nix {
            inherit pkgs;
            grabby = grabbyPkg;
          };
        };

        devShells.default = pkgs.callPackage ./nix/shell.nix {
          pre-commit-check = pre-commit-check;
          pkgs = pkgs;
        };

        checks = {
          pre-commit-check = pre-commit-check;
          package = grabbyPkg;
        };
      }
    );
}
