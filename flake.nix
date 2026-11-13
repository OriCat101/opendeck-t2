{
  description = "OpenDeck device plugin for the MacBook Touch Bar on Asahi Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # nixpkgs must follow: OpenDeck's webview has to share glibc with the system Mesa.
    opendeck = {
      url = "github:missmoode/nix-opendeck";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      treefmt-nix,
      opendeck,
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
      packages = forEachSystem (
        pkgs:
        let
          inherit (pkgs.stdenv.hostPlatform) system;
          opendeck-touchbar = pkgs.callPackage ./nix/package.nix {
            mkRustOpenDeckPlugin = opendeck.lib.mkRustOpenDeckPlugin pkgs;
          };
        in
        {
          inherit opendeck-touchbar;
          default = opendeck-touchbar;
          opendeck = import ./nix/opendeck.nix {
            inherit (opendeck.packages.${system}) opendeck-core;
            inherit (opendeck.legacyPackages.${system}) wrapOpenDeck;
            plugins = [ opendeck.legacyPackages.${system}.opendeck-plugins.nekename.starterpack ];
          };
        }
      );

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
              cp -rL --no-preserve=mode,ownership ${plugin}/opendeck-touchbar.sdPlugin "$dest"
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
    };
}
