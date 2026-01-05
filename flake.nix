{
  description = "MDValidate";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    treefmt.url = "github:numtide/treefmt-nix";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      treefmt,
      fenix,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        toolchain = fenix.packages.${system}.toolchainOf {
          channel = "1.90";
          sha256 = "sha256-SJwZ8g0zF2WrKDVmHrVG3pD2RGoQeo24MEXnNx5FyuI=";
        };

        treefmtEval = treefmt.lib.evalModule pkgs {
          projectRootFile = "flake.nix";
          programs.nixfmt.enable = true;
          programs.yamlfmt.enable = true;
          programs.typstyle.enable = true;
          programs.black.enable = true;
          programs.toml-sort.enable = true;
        };
      in
      {
        packages = rec {
          default = build;
          build = pkgs.callPackage ./nix/builds { };
          build-static = pkgs.callPackage ./nix/builds/static.nix { };
        };

        devShells.default = pkgs.mkShell {
          packages =
            (with pkgs; [
              nil
              nixd
              nixfmt
              typst
              mermaid-cli
              fira-mono
              cargo-release
              stdenv.cc.cc.lib
              bun
              just
            ])
            ++ [
              toolchain.defaultToolchain
            ];
          shellHook = ''
            export PATH=$PATH:target/debug
            export LD_LIBRARY_PATH=${pkgs.stdenv.cc.cc.lib}/lib:$LD_LIBRARY_PATH
          '';
        };

        formatter = treefmtEval.config.build.wrapper;

        checks = {
          formatting = treefmtEval.config.build.check self;
        };
      }
    );
}
