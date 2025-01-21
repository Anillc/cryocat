{
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  outputs = inputs@{
    self, nixpkgs, flake-parts, rust-overlay,
  }: flake-parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    perSystem = { self', pkgs, system, ... }: let
      rustPlatform = pkgs.makeRustPlatform {
        cargo = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default);
        rustc = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default);
      };
    in {
      _module.args.pkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
      };
      packages.cryocat = rustPlatform.buildRustPackage {
        name = "cryocat";
        src = ./.;
        buildAndTestSubdir = "cryocat";
        cargoHash = "sha256-atxcbxV0YHlTp98wKJi31wDYtsHtCaY2v6gkkP1VkLM=";
        auditable = false;
      };
      packages.cryocat-server = rustPlatform.buildRustPackage {
        name = "cryocat";
        src = ./.;
        buildAndTestSubdir = "cryocat-server";
        cargoHash = "sha256-atxcbxV0YHlTp98wKJi31wDYtsHtCaY2v6gkkP1VkLM=";
        auditable = false;
      };
      devShells.default = pkgs.mkShell {
        inputsFrom = with self'.packages; [ cryocat cryocat-server ];
      };
    };
  };
}
