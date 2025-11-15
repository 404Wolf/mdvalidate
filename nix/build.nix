{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mdvalidate";
  version = "0.1.0";

  src = ../.;

  cargoHash = "sha256-FlD8Gpk8PzB+7+MqKlnmSDSmzxpvMLcOBbHUgvP1hCc=";

  meta = {
    description = "Markdown Schema validator";
    homepage = "https://github.com/404Wolf/mdvalidate";
    license = lib.licenses.mit;
  };
}
