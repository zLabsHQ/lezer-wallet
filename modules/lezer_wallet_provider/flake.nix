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

      # rust-rapidsnark's build.rs downloads prebuilt archives unless
      # RAPIDSNARK_LIB_DIR is set. Mirror the URLs + digests upstream publishes in
      # logos-blockchain-rust-rapidsnark/{flake.nix,nix-hashes.json} (PIC archives
      # for Linux, iden3 upstream for macOS).
      rapidsnarkVersion = "0.0.8";
      rapidsnarkAssets = {
        aarch64-darwin = {
          url = "https://github.com/iden3/rapidsnark/releases/download/v${rapidsnarkVersion}/rapidsnark-macOS-arm64-v${rapidsnarkVersion}.zip";
          hash = "sha256-DmhMmMq7/HKsx+Iz4rdsKYBDEwqwMZALo+ZJePJvAJg=";
        };
        x86_64-linux = {
          url = "https://github.com/logos-blockchain/logos-blockchain-rust-rapidsnark/releases/download/rapidsnark-pic-v${rapidsnarkVersion}/rapidsnark-linux-x86_64-pic-v${rapidsnarkVersion}.zip";
          hash = "sha256-88+TkECQYCKBN0WbYLRB+qi6TEhbjVfrpCqlSgm0DR8=";
        };
        aarch64-linux = {
          url = "https://github.com/logos-blockchain/logos-blockchain-rust-rapidsnark/releases/download/rapidsnark-pic-v${rapidsnarkVersion}/rapidsnark-linux-aarch64-pic-v${rapidsnarkVersion}.zip";
          hash = "sha256-kcSITwWZjEv9gadzYL7LiG+DVtEEFbQJuhd/LTyOxJU=";
        };
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
          r = rapidsnarkAssets.${system};
          rapidsnarkLib = pkgs.stdenv.mkDerivation {
            pname = "rapidsnark-prebuilt";
            version = rapidsnarkVersion;
            phases = [ "installPhase" ];
            src = pkgs.fetchzip { inherit (r) url hash; };
            installPhase = ''
              mkdir -p $out && cp $src/lib/* $out/
            '';
          };
          # Upstream build_utils walks ancestors of the crate for artifacts/;
          # cargo vendor strips them, so we ship the v0.3.1 artifacts tree and
          # point the patched shim at it via LEE_ARTIFACTS_DIR (rustEnv below).
          lezTarball = pkgs.fetchurl {
            url = "https://github.com/logos-blockchain/logos-execution-zone/archive/refs/tags/v0.3.1.tar.gz";
            sha256 = "7b5a7be99fee3594b148f064f14bca8d4bbc824138f6283518bb20cf381e84c0";
          };
          leeArtifacts = pkgs.runCommand "lez-v0.3.1-artifacts" { } ''
            mkdir -p $TMP/src && tar -xzf ${lezTarball} -C $TMP/src
            mkdir -p $out && cp -r $TMP/src/*/artifacts $out/artifacts
          '';
        in
        logos-module-builder.lib.mkLogosModule {
          src = ./.;
          configFile = ./metadata.json;
          flakeInputs = inputs;
          rustEnv = {
            LBC_ROOT_DIR = "${circuitsRoot}";
            RAPIDSNARK_LIB_DIR = "${rapidsnarkLib}";
            LEE_ARTIFACTS_DIR = "${leeArtifacts}/artifacts";
          };
          # librocksdb-sys/risc0 need libclang (bindgen); keycard needs pcsc on Linux.
          rustExtraNativeBuildInputs = [ pkgs.rustPlatform.bindgenHook pkgs.pkg-config ];
          rustExtraBuildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.pcsclite ];
        };
    in
    {
      packages = lib.genAttrs systems (system: (moduleFor system).packages.${system});
    };
}
