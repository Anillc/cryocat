{ stdenv, rustPlatform }:

rustPlatform.buildRustPackage {
  name = "cryocat-server";
  src = ../.;
  buildAndTestSubdir = "cryocat-server";
  cargoHash = "sha256-gxcVKCy0HNYwTTsRN+npXRc0O1yhEnEgir1BR6gsy7U=";
  RUSTC_BOOTSTRAP = 1;
}
