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
          programs.typstfmt.enable = true;
        };
      in
      {
        packages = rec {
          default = build;
          build = pkgs.callPackage ./nix/build.nix { };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            nil
            nixd
            nixfmt
            typst
            cargo
            rustc
            mermaid-cli
            rust-analyzer
            fira-mono
          ];
        };

        formatter = treefmtEval.config.build.wrapper;

        checks = {
          # formatting = treefmtEval.config.build.check self;
          clippy =
            let
              cargoVendorDeps = pkgs.rustPlatform.fetchCargoVendor {
                name = "mdvalidate-vendor";
                hash = "sha256-PFkpHwe/L7okA+p4Zzj917Wb5gNRJJ9Ei0MTg2xMlVI=";
                src = ./.;
              };
            in
            pkgs.runCommandLocal "clippy-check"
              {
                src = ./.;
                nativeBuildInputs = with pkgs; [
                  cargo
                  rustc
                  clippy
                ];
                CARGO_HOME = cargoVendorDeps;
              }
              ''
                cp -r ${./.} .
                cd *source
                export CARGO_HOME=${cargoVendorDeps}
                export CARGO_NET_OFFLINE=true
                cargo clippy --all-targets --all-features -- -D warnings
                mkdir $out
              '';
        };
      }
    );
}
