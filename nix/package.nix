{
  lib,
  callPackage,
  mkRustOpenDeckPlugin,
}:

mkRustOpenDeckPlugin (finalAttrs: {
  pname = "opendeck-touchbar";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../assets
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  pluginId = "opendeck-touchbar.sdPlugin";
  binaryName = "opendeck-touchbar";
  binaryInstallPath = "opendeck-touchbar";
  licenseFiles = null;

  udevRules."70-opendeck-touchbar.rules" = callPackage ./udev-rules.nix { };

  postInstall = ''
    mkdir -p $out/bin
    ln -s $out/${finalAttrs.pluginId}/opendeck-touchbar $out/bin/opendeck-touchbar
  '';

  meta = {
    description = "OpenDeck device plugin for the MacBook Touch Bar on Asahi Linux";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "opendeck-touchbar";
  };
})
