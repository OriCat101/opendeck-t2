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
    postPatch = old.postPatch + ''
      substituteInPlace src/components/Key.svelte \
        --replace-fail \
        'export let width: number = size;' \
        'export let width: number = context?.device.startsWith("tb-") ? 327 : size;'

      substituteInPlace src/lib/rendererHelper.ts \
        --replace-fail \
        'const xScaled = canvas.width * imageScale;' \
        'const xScaled = (slotContext?.device.startsWith("tb-") && image.naturalWidth > 1 ? canvas.height : canvas.width) * imageScale;'
    '';
  });
}
