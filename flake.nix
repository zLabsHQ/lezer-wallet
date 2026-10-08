{
  description = "Lezer Wallet Suite — Native Rust LEZ Wallet, Basecamp Core & QML Modules, CLI, and SDK";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    nix-bundle-lgx.url = "github:logos-co/nix-bundle-lgx";
    nixpkgs.follows = "logos-module-builder/nixpkgs";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    lezer_wallet_provider = {
      url = "path:./modules/lezer_wallet_provider";
      inputs.logos-module-builder.follows = "logos-module-builder";
      inputs.nix-bundle-lgx.follows = "nix-bundle-lgx";
      inputs.rust-overlay.follows = "rust-overlay";
    };
    lezer_wallet_ui = {
      url = "path:./modules/lezer_wallet_ui";
      inputs.logos-module-builder.follows = "logos-module-builder";
      inputs.nix-bundle-lgx.follows = "nix-bundle-lgx";
      inputs.lezer_wallet_provider.follows = "lezer_wallet_provider";
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    lezer_wallet_provider,
    lezer_wallet_ui,
    ...
  }:
    let
      lib = nixpkgs.lib;
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAllSystems = lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system: {
        lezer_wallet_provider = lezer_wallet_provider.packages.${system}.default;
        lezer_wallet_ui = lezer_wallet_ui.packages.${system}.default;
        default = lezer_wallet_ui.packages.${system}.default;
      });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable."1.93.0".default;
        in
        {
          default = pkgs.mkShell {
            buildInputs = [
              rustToolchain
              pkgs.pkg-config
              pkgs.pcsclite
              pkgs.nodejs_22
              pkgs.jq
            ];
          };
        }
      );
    };
}
