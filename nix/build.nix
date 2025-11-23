{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mdvalidate";
  version = "0.1.0";

  src = ../.;

  cargoHash = "sha256-2p5wsu9+/DF9DBpwBCxQWGa/CsUffbfT4Egk2Rltzag=";

  meta = {
    description = "Markdown Schema validator";
    homepage = "https://github.com/404Wolf/mdvalidate";
    license = lib.licenses.mit;
  };
}
