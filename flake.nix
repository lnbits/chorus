{
  description = "Chorus — federated deterministic WASM attestations";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));
    in {
      packages = forAllSystems (pkgs: rec {
        node = pkgs.rustPlatform.buildRustPackage {
          pname = "chorus-node";
          version = "0.1.0";
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type: let name = baseNameOf path; in
              !(builtins.elem name [ "target" "node_modules" ".svelte-kit" "build" ".git" "data" "test-results" ]);
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "chorus-node" "-p" "chorus-verifier" ];
          cargoTestFlags = [ "--workspace" ];
          meta.mainProgram = "chorus-node";
        };
        web = pkgs.buildNpmPackage {
          pname = "chorus-web";
          version = "0.1.0";
          src = ./apps/web;
          npmDeps = pkgs.importNpmLock { npmRoot = ./apps/web; };
          npmConfigHook = pkgs.importNpmLock.npmConfigHook;
          installPhase = "mkdir -p $out; cp -r build/. $out/";
        };
        default = node;
      });
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc rustfmt clippy cargo-audit nodejs_24 pkg-config openssl sqlite ]
            ++ lib.optionals stdenv.hostPlatform.isLinux [ webkitgtk_4_1 gtk3 libsoup_3 ];
        };
      });
      nixosModules.default = import ./deploy/nixos-module.nix self;
    };
}
