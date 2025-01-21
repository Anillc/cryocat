{ stdenv, rustPlatform }:

rustPlatform.buildRustPackage {
  name = "cryocat";
  src = ../.;
  buildAndTestSubdir = "cryocat";
  cargoHash = "sha256-ZeIUD+GgPXvJsQMkE9GLJrCf1PlJCcKcIyxkcTAl058=";
  RUSTC_BOOTSTRAP = 1;
}
