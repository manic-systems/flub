{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?ref=nixos-unstable";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, ... }@inputs:
    let
      inherit (inputs) nixpkgs fenix;
      inherit (nixpkgs) lib;
      forAllSystems = lib.genAttrs (lib.systems.doubles.linux ++ lib.systems.doubles.darwin);
      pkgsFor = system: nixpkgs.legacyPackages.${system} or (import nixpkgs { inherit system; });
    in
    {
      packages = forAllSystems (
        system:
        let
          flub = (pkgsFor system).callPackage ./nix/package.nix { };
        in
        {
          inherit flub;
          default = flub;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.callPackage ./nix/shell.nix {
            rustfmt = fenix.packages.${system}.latest.rustfmt or pkgs.rustfmt;
          };
        }
      );

      checks = forAllSystems (system: {
        flub = self.packages.${system}.flub;
      });
    };
}
