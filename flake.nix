{
  description = "ArkDB: the specification, the toolchain, the runtimes, and harken on them";

  # FlakeHub rather than github: for every input, because a machine that can
  # reach FlakeHub and cache.nixos.org but not api.github.com (this one, for
  # instance) must still be able to build. The lock pins the tarballs.
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2411.*.tar.gz";
    rust-overlay.url = "https://flakehub.com/f/oxalica/rust-overlay/0.1.*.tar.gz";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system:
        f (import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; }));
    in
    {
      packages = forAll (pkgs: rec {
        # The specification and arkc, one Haskell package: `ark-spec` holds
        # the library and both executables.
        ark-spec = pkgs.haskellPackages.callCabal2nix "ark-spec" ./spec { };
        # Regenerates spec/vectors from the specification.
        vectors = pkgs.writeShellApplication {
          name = "ark-vectors";
          runtimeInputs = [ ark-spec ];
          text = ''exec ark-vectors "$@"'';
        };
        # The toolchain: verify, print, hash, check, gen.
        arkc = pkgs.writeShellApplication {
          name = "arkc";
          runtimeInputs = [ ark-spec ];
          text = ''exec arkc "$@"'';
        };
        default = ark-spec;
      });

      devShells = forAll (pkgs:
        let
          rust = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" ];
          };
        in
        {
          # The spec: GHC with the boot libraries the spec confines itself to.
          spec = pkgs.mkShell {
            packages = [
              (pkgs.haskellPackages.ghcWithPackages (p: [ p.array p.bytestring p.containers p.text p.mtl p.directory ]))
              pkgs.cabal-install
              pkgs.haskell-language-server
              pkgs.ormolu
            ];
          };
          rust = pkgs.mkShell {
            packages = [ rust pkgs.pkg-config pkgs.sqlite pkgs.openssl ];
          };
          swift = pkgs.mkShell {
            packages = [ pkgs.swift pkgs.swiftpm pkgs.swiftPackages.Foundation pkgs.sqlite ];
          };
          kotlin = pkgs.mkShell {
            packages = [ pkgs.kotlin pkgs.gradle pkgs.jdk21 ];
          };
          default = pkgs.mkShell {
            packages = [ pkgs.cabal-install rust pkgs.kotlin pkgs.jdk21 ];
          };
        });
    };
}
