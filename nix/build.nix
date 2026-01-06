{
  lib,
  pkgs,
  static ? true,
}:
pkgs.pkgsCross.musl64.rustPlatform.buildRustPackage (
  {
    pname = "mdvalidate";
    version = "0.1.0";

    src = ../.;

    cargoHash = "sha256-M/uvId8PnqvLPkYWuZFqjzzFJ4Ip5zqdbMt8cALfmuI=";

    cargoBuildFlags = [
      "--bin"
      "mdv"
    ];

    meta = {
      description = "Markdown Schema validator";
      homepage = "https://github.com/404Wolf/mdvalidate";
      license = lib.licenses.mit;
    };
  }
  // lib.optionalAttrs static {
    env = {
      RUSTFLAGS = "-C target-feature=+crt-static -C link-self-contained=yes";
    };

    CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
  }
)
