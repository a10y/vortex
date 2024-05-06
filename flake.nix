{
  description = "Vortex memory format";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
    zig = {
      url = "github:mitchellh/zig-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs = { self , nixpkgs , flake-utils , rust-overlay , zig , ... }:
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          overlays = [ 
            (import rust-overlay)
            # Zig overlay
            (final: prev: {
              zigpkgs = zig.packages.${prev.system};
            })
          ];
          pkgs = import nixpkgs {
            inherit system overlays;
          };
          rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          zigToolchain = with pkgs; (callPackage ./nix/zig-toolchains.nix {
            inherit pkgs;
          }).findByVersion "0.12.0-dev.2542+5a3ae38f3";

          # Optional Apple frameworks
          optional-frameworks = with pkgs; lib.optionals stdenv.isDarwin [
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
          ];
        in
        {
          devShells.default = pkgs.mkShell {
            buildInputs = [ rustToolchain zigToolchain ] ++ optional-frameworks;
          };
        }
      );
}
