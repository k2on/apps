{
  description = "The ArkDB specification, as a Haskell program";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAll (pkgs: rec {
        ark-spec = pkgs.haskellPackages.callCabal2nix "ark-spec" ./. { };
        default = ark-spec;
      });
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [ pkgs.ghc pkgs.cabal-install pkgs.haskell-language-server ];
        };
      });
    };
}
