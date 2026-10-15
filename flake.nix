{
  description = "Linux CLI for the Nothing Headphone (1)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      cargo = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAll (pkgs: rec {
        nothing-toolbox = pkgs.rustPlatform.buildRustPackage {
          pname = "nothing-toolbox";
          version = cargo.package.version;
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.installShellFiles ];
          # The tool finds the connected headset through bluetoothctl.
          postInstall = ''
            installShellCompletion --cmd nothing \
              --bash <($out/bin/nothing completions bash) \
              --zsh <($out/bin/nothing completions zsh) \
              --fish <($out/bin/nothing completions fish)
          '';
          meta = {
            description = cargo.package.description;
            homepage = "https://github.com/marc-alexis-com/nothing-toolbox";
            license = pkgs.lib.licenses.mit;
            mainProgram = "nothing";
            platforms = pkgs.lib.platforms.linux;
          };
        };
        default = nothing-toolbox;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy ];
        };
      });
    };
}
