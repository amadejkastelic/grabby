{
  description = "Grabby - Media Embedding Discord Bot";

  nixConfig = {
    extra-substituters = [ "https://amadejkastelic.cachix.org" ];
    extra-trusted-public-keys = [
      "amadejkastelic.cachix.org-1:EiQfTbiT0UKsynF4q3nbNYjNH6/l7zuhrNkQTuXmyOs="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";

    pre-commit-hooks = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-github-actions = {
      url = "github:nix-community/nix-github-actions";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
      flake-utils,
      pre-commit-hooks,
      nix-github-actions,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        rustPlatform = pkgs.makeRustPlatform {
          inherit (fenix.packages.${system}.stable) rustc cargo;
        };

        devToolchain = fenix.packages.${system}.stable.withComponents [
          "cargo"
          "clippy"
          "rust-src"
          "rustc"
          "rustfmt"
        ];

        grabbyPkg = pkgs.callPackage ./nix/package.nix { inherit rustPlatform; };

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
          settings.rust.check.cargoDeps = rustPlatform.importCargoLock {
            lockFile = ./Cargo.lock;
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
          inherit rustPlatform devToolchain pre-commit-check;
          pkgs = pkgs;
        };

        checks = {
          pre-commit-check = pre-commit-check;
        };
      }
    )
    // {
      nixosModules.grabby =
        {
          config,
          pkgs,
          lib,
          ...
        }:
        import ./nix/module.nix {
          inherit config pkgs lib;
          package = self.packages.${pkgs.stdenv.hostPlatform.system}.grabby;
        };

      githubActions = nix-github-actions.lib.mkGithubMatrix {
        inherit (self) checks;
      };
    };
}
