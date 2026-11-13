{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "opendeck-touchbar";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../manifest.json
      ../assets
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  postInstall = ''
    plugin=$out/share/opendeck-touchbar/opendeck-touchbar.sdPlugin
    mkdir -p $plugin
    cp -r manifest.json assets $plugin/
    cp $out/bin/opendeck-touchbar $plugin/opendeck-touchbar
  '';

  meta = {
    description = "OpenDeck device plugin for the MacBook Touch Bar on Asahi Linux";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "opendeck-touchbar";
  };
}
