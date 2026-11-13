# OpenDeck with a frontend patch: keys of Touch Bar (`tb-`) devices are rendered
# on a wide canvas matching the bar's buttons instead of a 144x144 square.
{
  opendeck-core,
  wrapOpenDeck,
  plugins,
}:
wrapOpenDeck {
  pname = "opendeck";
  inherit plugins;
  package = opendeck-core.overrideAttrs (old: {
    patches = old.patches ++ [ ./patches/opendeck-wide-keys.patch ];
  });
}
