{
  description = "Browser Phone - Android in the browser with WebRTC";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    typix.url = "github:loqusion/typix";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    treefmt.url = "github:numtide/treefmt-nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      typix,
      flake-utils,
      treefmt,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };

        watch-script = typix.lib.${system}.watchTypstProject typix-config;
        typix-config = {
          typstSource = "./srs.typ";
          fontPaths = [ "${pkgs.roboto}/share/fonts/truetype" ];
        };
      in
      {
        packages = {
          default = pkgs.callPackage ./android/docker.nix { inherit pkgs; };
          srs = typix.lib.${system}.buildTypstProject typix-config;
        };

        apps = {
          srs = {
            type = "app";
            program = pkgs.lib.getExe watch-script;
          };
          build-diagrams = {
            type = "app";
            program = "${
              (pkgs.writeShellApplication {
                name = "build-diagrams";
                runtimeInputs = [ pkgs.mermaid-cli ];
                text = builtins.readFile ./images/build.sh;
              })
            }/bin/build-diagrams";
          };
        };

        formatter =
          let
            treefmtconfig = treefmt.lib.evalModule pkgs {
              projectRootFile = "flake.nix";
              programs.typstyle.enable = true;
              programs.nixfmt.enable = true;
              programs.google-java-format.enable = true;
            };
          in
          treefmtconfig.config.build.wrapper;

        devShells.default = typix.lib.${system}.devShell {
          fontPaths = [ "${pkgs.roboto}/share/fonts/truetype" ];
          packages =
            [ watch-script ]
            ++ (with pkgs; [
              nil
              tcpdump
              scrcpy
              mermaid-cli
              android-tools
              nixfmt
            ]);
        };
      }
    );
}
