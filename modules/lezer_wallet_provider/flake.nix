{
  description = "Lezer wallet provider — LEZ wallet Basecamp core module (Rust, codegen.rust)";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/8f56bf8";
    # The builder's locked rust-overlay (Aug 2026) predates Rust 1.98.1, which
    # `nix.rust.toolchain` selects; point it at a rev that knows 1.98.1.
    rust-overlay.url = "github:oxalica/rust-overlay/6ecf534a7c16e7c969f26838fdd8c93597ae93f3";
    logos-module-builder.inputs.rust-overlay.follows = "rust-overlay";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    let
      nixpkgs = logos-module-builder.inputs.nixpkgs;
      lib = nixpkgs.lib;

      # logos-execution-zone v0.3.1 links logos-blockchain-circuits v0.5.7. Its build
      # script downloads these archives unless LBC_ROOT_DIR is set; the Nix sandbox has
      # no network, so they are fetched here, pinned to GitHub's published digests.
      # (No macOS x86_64 archive is published upstream, so that system is not offered.)
      circuitsVersion = "0.5.7";
      circuits = {
        aarch64-darwin = { platform = "macos-aarch64"; sha256 = "75a52885bf3b3aad513457f929fe4842cd51b3c46226e145f21a0858bb8b4b25"; };
        x86_64-linux = { platform = "linux-x86_64"; sha256 = "92a0c6ad810d486631518b7b9b247a2b66f66af6f8f8ddce381acbbdfedc0f7f"; };
        aarch64-linux = { platform = "linux-aarch64"; sha256 = "1eaa3e339d85d741dd70314c4ad45b7ee67c2baaf6c55e35170801161c372d7b"; };
      };
      systems = builtins.attrNames circuits;

      moduleFor = system:
        let
          pkgs = import nixpkgs { inherit system; };
          c = circuits.${system};
          name = "logos-blockchain-circuits-v${circuitsVersion}-${c.platform}";
          tarball = pkgs.fetchurl {
            url = "https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/v${circuitsVersion}/${name}.tar.gz";
            inherit (c) sha256;
          };
          circuitsRoot = pkgs.runCommand name { } ''
            mkdir -p $out && tar -xzf ${tarball} -C $out --strip-components=1
          '';
        in
        logos-module-builder.lib.mkLogosModule {
          src = ./.;
          configFile = ./metadata.json;
          flakeInputs = inputs;
          rustEnv = { LBC_ROOT_DIR = "${circuitsRoot}"; };
          # librocksdb-sys/risc0 need libclang (bindgen); keycard needs pcsc on Linux.
          rustExtraNativeBuildInputs = [ pkgs.rustPlatform.bindgenHook pkgs.pkg-config ];
          rustExtraBuildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.pcsclite ];
        };
    in
    {
      packages = lib.genAttrs systems (system: (moduleFor system).packages.${system});
    };
}
