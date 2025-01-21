{
  outputs = inputs@{
    self, nixpkgs, flake-parts,
  }: let
    # use overlay to get cross compiling work.
    overlay = self: super: {
      cryocat = self.callPackage ({ rustPlatform }: rustPlatform.buildRustPackage {
        name = "cryocat";
        src = ./.;
        cargoHash = "sha256-ZeIUD+GgPXvJsQMkE9GLJrCf1PlJCcKcIyxkcTAl058=";
        RUSTC_BOOTSTRAP = 1;
      }) {};
    };
  in flake-parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    perSystem = { self', self, pkgs, system, ... }: {
      _module.args.pkgs = import nixpkgs {
        inherit system;
        overlays = [ overlay ];
      };
      packages = rec {
        inherit (pkgs) cryocat;
        static-cryocat = pkgs.pkgsStatic.cryocat;
        default = cryocat;
      };
      devShells.default = pkgs.mkShell {
        inputsFrom = with self'.packages; [ default ];
        RUSTC_BOOTSTRAP = 1;
      };
    };
  };
}
