{
  description = "MDValidate";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    treefmt.url = "github:numtide/treefmt-nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      treefmt,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        treefmtEval = treefmt.lib.evalModule pkgs {
          projectRootFile = "flake.nix";
          programs.nixfmt.enable = true;
          programs.yamlfmt.enable = true;
          programs.typstyle.enable = true;
          programs.toml-sort.enable = true;
        };
      in
      {
        packages = rec {
          default = build;
          build = pkgs.callPackage ./nix/build.nix { };
        };

        devShells.default = pkgs.mkShell {
          packages = (
            with pkgs;
            [
              perf
              nil
              nixd
              nixfmt
              typst
              cargo
              rustc
              mermaid-cli
              rust-analyzer
              fira-mono
              git-cliff
              cargo-release
            ]
          );
          shellHook = ''
            export PATH=$PATH:target/debug
            export LLVM_COV=${pkgs.llvmPackages_latest.llvm}/bin/llvm-cov
            export LLVM_PROFDATA=${pkgs.llvmPackages_latest.llvm}/bin/llvm-profdata
          '';
        };

        formatter = treefmtEval.config.build.wrapper;

        checks = {
          formatting = treefmtEval.config.build.check self;
        };
      }
    );
}
