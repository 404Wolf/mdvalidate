{
  lib,
  rustPlatform,
  static ? false,
}:
rustPlatform.buildRustPackage {
  pname = "mdvalidate";
  version = "0.1.0";

  src = ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  cargoBuildFlags = [
    "--bin"
    "mdv"
  ];

  # TODO: for now, until we get them all passing!
  doCheck = false;

  meta = {
    description = "Markdown Schema validator";
    homepage = "https://github.com/404Wolf/mdvalidate";
    license = lib.licenses.mit;
  };
}
// (lib.optionalAttrs static {
  env = {
    RUSTFLAGS = "-C target-feature=+crt-static -C link-self-contained=yes";
  };

  CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
})
