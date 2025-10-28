{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mdvalidate";
  version = "0.1.0";

  src = ../.;

  cargoHash = "sha256-KTIr7XgNRksysOL2DhayZgyA+ZDwRNfIXj31AHVQzu0=";

  meta = {
    description = "Markdown Schema validator";
    homepage = "https://github.com/404Wolf/mdvalidate";
    license = lib.licenses.mit;
  };
}
