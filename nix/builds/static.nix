{ lib, pkgs }:
pkgs.pkgsCross.musl64.rustPlatform.buildRustPackage {
  pname = "mdvalidate";
  version = "0.1.0";

  src = ../../.;

  cargoHash = "sha256-HCxLc3M3Tza1pVlLOPgmiZu6Ra2oFzSCmwSSTuQW+u0=";

  env = {
    RUSTFLAGS = "-C target-feature=+crt-static -C link-self-contained=yes";
  };

  CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";

  meta = {
    description = "Markdown Schema validator";
    homepage = "https://github.com/404Wolf/mdvalidate";
    license = lib.licenses.mit;
  };
}
