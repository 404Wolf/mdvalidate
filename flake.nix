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
      in
      {
        packages = {
          default = pkgs.callPackage ./android/docker.nix { inherit pkgs; };
        };

        formatter =
          let
            treefmtconfig = treefmt.lib.evalModule pkgs {
              projectRootFile = "flake.nix";
              programs.nixfmt.enable = true;
              programs.prettier.enable = true;
            };
          in
          treefmtconfig.config.build.wrapper;

        devShells.default = pkgs.mkShell {
          packages = (
            with pkgs;
            [
              nil
              nixd
              nixfmt
              quarto
              texliveSmall
            ]
          );
        };
      }
    );
}
