{
  description = "OpenDeck device plugin for the MacBook Touch Bar on Asahi Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # No nixpkgs follows: its fixed-output hashes depend on its own pin.
    opendeck-nix.url = "github:Kitt3120/opendeck-nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      treefmt-nix,
      opendeck-nix,
    }:
    let
      systems = [
        "aarch64-linux"
        "x86_64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      treefmtEval = forEachSystem (pkgs: treefmt-nix.lib.evalModule pkgs ./treefmt.nix);
    in
    {
      packages = forEachSystem (pkgs: rec {
        opendeck-touchbar = pkgs.callPackage ./nix/package.nix { };
        default = opendeck-touchbar;
        # Built with our nixpkgs, not opendeck-nix's pin: the webview must share glibc with the system Mesa.
        opendeck = (pkgs.callPackage "${opendeck-nix}/pkg/package.nix" { }).overrideAttrs (old: {
          buildInputs = old.buildInputs ++ [ pkgs.libgit2 ];
        });
      });

      apps = forEachSystem (
        pkgs:
        let
          plugin = self.packages.${pkgs.stdenv.hostPlatform.system}.opendeck-touchbar;
          install = pkgs.writeShellApplication {
            name = "install-opendeck-touchbar";
            text = ''
              dest="''${XDG_CONFIG_HOME:-$HOME/.config}/opendeck/plugins/opendeck-touchbar.sdPlugin"
              mkdir -p "$(dirname "$dest")"
              rm -rf "$dest"
              cp -rL --no-preserve=mode,ownership ${plugin}/share/opendeck-touchbar/opendeck-touchbar.sdPlugin "$dest"
              chmod 0755 "$dest/opendeck-touchbar"
              echo "installed to $dest"
            '';
          };
        in
        {
          install = {
            type = "app";
            program = "${install}/bin/install-opendeck-touchbar";
          };
        }
      );

      formatter = forEachSystem (
        pkgs: treefmtEval.${pkgs.stdenv.hostPlatform.system}.config.build.wrapper
      );

      checks = forEachSystem (pkgs: {
        formatting = treefmtEval.${pkgs.stdenv.hostPlatform.system}.config.build.check self;
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.opendeck-touchbar;
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rust-analyzer
            treefmtEval.${pkgs.stdenv.hostPlatform.system}.config.build.wrapper
          ];
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      nixosModules.default = import ./nix/module.nix;
    };
}
