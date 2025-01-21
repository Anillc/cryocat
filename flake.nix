{
  outputs = inputs@{
    self, nixpkgs, flake-parts,
  }: flake-parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    perSystem = { self', pkgs, system, ... }: {
      _module.args.pkgs = import nixpkgs {
        inherit system;
        overlays = [
          (self: super: {
            cryocat = self.callPackage ./nix/cryocat.nix {};
            cryocat-server = self.callPackage ./nix/cryocat-server.nix {};
          })
        ];
      };
      packages = {
        inherit (pkgs) cryocat cryocat-server;
        static-cryocat = pkgs.pkgsStatic.cryocat;
        static-cryocat-server = pkgs.pkgsStatic.cryocat-server;
      };
      devShells.default = pkgs.mkShell {
        inputsFrom = with self'.packages; [ cryocat cryocat-server ];
        RUSTC_BOOTSTRAP = 1;
      };
    };
  };
}
