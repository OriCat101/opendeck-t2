# opendeck-touchbar

OpenDeck device plugin that turns the MacBook Touch Bar into a deck on Asahi Linux (NixOS, aarch64). One Rust binary owns the bar directly and replaces tiny-dfr: no function row, bar is blank when OpenDeck is not running. Target machine: MacBookPro17,1 (M1 13", j293).

## Status

- `--demo` (numbered keys, prints touches) verified on hardware as root with tiny-dfr stopped: display, orientation, key layout and touch mapping are correct without `OPENDECK_TOUCHBAR_FLIP`.
- Non-root works: with tiny-dfr stopped and the device nodes owned by the user, the demo and the plugin take the display without root (first opener becomes DRM master). Ad-hoc setup until reboot: `sudo chown ori /dev/dri/card2 /dev/input/event3 /sys/class/backlight/228600000.dsi.0/brightness`.
- The plugin connects to OpenDeck and registers its 16-key device (seen in the plugin log).
- The udev rules output (the permanent form of that access) is tested and works.
- Keyboard, trackpad and mouse activity wakes the bar and keeps it on, via the compositor's `ext-idle-notify-v1` (`src/bar/activity.rs`; no device access). It is a plugin global setting (`wake_on_input`, default on) toggled by the plugin's settings button in OpenDeck. Verified on hardware (mango). An earlier evdev version (reading the keyboard/trackpad nodes through a `setfacl` udev rule) opened the devices but never woke the bar on the user's machine (compositor: mango); cause unknown.
- Not yet tested: backlight/idle dimming behaviour, wake from suspend.
- OpenDeck comes from `github:missmoode/nix-opendeck` (patched to load plugins from the Nix store; also provides the home-manager module). It builds and runs on aarch64 (2.14.2), and the plugin loads through its home-manager module as a store symlink (confirmed on yuriBook).
- `.#opendeck` is nix-opendeck's core plus our frontend patch (`nix/patches/opendeck-wide-keys.patch`, a git diff against OpenDeck's source, applied in `nix/opendeck.nix`). Users opt in with `programs.opendeck.package`. What it does:
  - Keys of `tb-` devices render on a 327x144 canvas (3x the 109x48 button). Verified on hardware.
  - Images are fitted into the canvas keeping their proportions (contain), on all devices. 1x1 images fill the canvas: OpenDeck's "solid colour" button stores a 1x1 image override, not a background colour.
  - Picked images are stored with their own proportions (fitted into 654x288) instead of letterboxed into 288x288. Images picked before the patch keep their baked-in bars and must be picked again.
  - OK/alert overlays are drawn square.
  - The action list sidebar gets `shrink-0`: the Touch Bar's key row is about 5000 px wide and would otherwise squeeze it. nix-opendeck's patch 0005 touches the same file (`ActionList.svelte`); ours applies on top with an offset.
  - To change the patch: copy the touched files from the OpenDeck source in the Nix store, edit, regenerate with `git diff --no-index`, and check with `git apply --check`.
- The `opendeck` input's nixpkgs must follow ours/the system's: the webview aborts with `Could not create default EGL display: EGL_BAD_PARAMETER` when its glibc is older than the system Mesa's (seen with an older pin).
- OpenDeck's panic at `src/main.rs:271` on startup is its update check hitting the GitHub rate limit; harmless.

## Build and check

- Build with `nix build .#opendeck-touchbar`; lint with `nix develop -c cargo clippy --all-targets`.
- Formatting is enforced by treefmt (rustfmt + nixfmt): `nix fmt` formats, `nix flake check` fails on unformatted code.
- Nix only sees git-tracked files: `git add` new files before building. Add files by name, never `git add -A` (the sandbox shows fake dotfiles like `.bash_profile` in the repo root).
- The unauthenticated GitHub API gets rate-limited: resolve revisions with `, gh api repos/OWNER/REPO/commits/REF --jq .sha` and lock with `nix flake lock --override-input NAME github:OWNER/REPO/SHA`.
- The agent sandbox has no `/dev/dri` or `/dev/input`, and stopping tiny-dfr needs root, so hardware tests are run by the user:
  `sudo systemctl stop tiny-dfr; sudo ./result/bin/opendeck-touchbar --demo` (restore with `sudo systemctl start tiny-dfr`).

## Touch Bar hardware (Apple Silicon)

- Display: DRM card driven by `adp` (found via `/sys/class/drm/cardN/device/driver`, currently `card2`, connector `DSI-1`). Single mode 60x2008 (portrait), XRGB8888 only (bytes B,G,R,X).
- Drawing recipe (same as tiny-dfr): client caps UniversalPlanes + Atomic, dumb buffer 64x2008 at 32 bpp, `add_framebuffer(db, 24, 32)`, first connector/crtc/plane, one `ALLOW_MODESET` atomic commit, then write into the mapped buffer and call `dirty_framebuffer`. No page flips.
- Rotation: logical landscape pixel `(x, y)` on the 2008x60 bar maps to framebuffer `(59 - y, x)`. With the flip env var it is `(y, 2007 - x)`.
- Touch: evdev device named `MacBookPro17,1 Touch Bar` (driver `apple_z2`, currently `event3`), MT slots, `ABS_MT_POSITION_X` 0..23044 runs along the bar left to right. The device is grabbed.
- Backlight: `/sys/class/backlight/228600000.dsi.0/brightness`, 0..255, root-writable by default. The panel is OLED, hence dim after 30 s and off after 60 s without touch, keyboard or pointer activity.
- The adp driver refuses opens from processes whose name starts with `X`.
- Both devices carry `ID_SEAT=seat-touchbar` from udev rules shipped with tiny-dfr; that keeps the compositor off them. `nix/udev-rules.nix` re-ships those rules (they vanish when tiny-dfr is disabled) and grants the `touchbar` group access.- T2 Macs use a different stack (`appletbdrm`, `hid-appletb-*`); driver names are listed in the code but nothing there is tested.

## OpenDeck device plugin protocol (OpenDeck 2.14, `openaction` 2.8)

- OpenDeck launches the binary with `-port -pluginUUID -registerEvent -info`; `openaction::run(args)` handles the websocket. Handlers come from one `GlobalEventHandler` set with `set_global_event_handler` before `run`.
- `device_plugin::register_device(id, name, rows, columns, encoders, type)`; `type` is not interpreted (0). Keys are addressed by linear position. `key_down` / `key_up` / `unregister_device` take the device id.
- The first two characters of the device id must equal the manifest `DeviceNamespace` (`tb`; device id `tb-touchbar`).
- `device_plugin_set_image`: `image` is a `data:image/jpeg;base64` URL of a 144x144 key (327x144 with our patched OpenDeck; the size is whatever the key canvas in OpenDeck's window is, there is no protocol field for it). The plugin fits any aspect into the button. `image: None` clears one key; `position: None` clears the whole device. `controller` is `Keypad`, `Encoder` or `Infobar`.
- `device_plugin_set_brightness`: percentage.
- Settings: OpenDeck has no settings page for device plugins. Manifest `HasSettingsInterface: true` adds a settings button to the plugin list which only sends `showSettingsInterface`; the plugin treats it as a toggle. Global settings (`get_global_settings` / `set_global_settings` / `did_receive_global_settings`) are stored in `~/.config/opendeck/settings/<plugin id>.json`.
- Images can arrive immediately after registration, so the bar must be ready first.
- Manifest: `Actions: []`, `OS: [{Platform: linux}]`, `CodePathLin`, and `CodePaths` keyed by Rust target triple (`aarch64-unknown-linux-gnu`).
- Plugins live in `~/.config/opendeck/plugins/<dir>`; the directory name is the plugin UUID. A stock OpenDeck `chmod 0755`s the binary before launch and refuses to serve plugin assets outside the config dir, so there the plugin must be a real user-owned copy (`nix run .#install`). nix-opendeck's patched OpenDeck loads plugins as symlinks into the Nix store.
- Plugin logs: `~/.local/share/opendeck/logs/plugins/opendeck-touchbar.sdPlugin.log`.
- Reference plugin on openaction 2.x: `ambiso/opendeck-akp05`. `4ndv/opendeck-akp03` is still on the incompatible 1.x API.

## Code map

- `src/bar/display.rs` DRM setup and rotated drawing; `src/bar/touch.rs` evdev slots to touch events; `src/bar/backlight.rs`; `src/bar/activity.rs` runs a Wayland client thread (`ext-idle-notify-v1`, 1 s timeout, `get_input_idle_notification` on v2) and reports `resumed`/`idled` through `Bar::set_input_active()`; `src/bar/mod.rs` the `Bar` (keys drawn as tiny-dfr style rounded buttons with press highlight, brightness, idle power) and `start()` which yields `KeyEvent`s. Drawing only writes the buffer; a presenter task flushes once changes have settled for 20 ms (150 ms at most), so a new layer (OpenDeck sends a clear plus one message per key) appears in one step. Presses flush at once. Verified on hardware.
- `src/layout.rs` key grid (`COLS = 16`, one row); `src/demo.rs` hardware test; `src/plugin.rs` OpenDeck glue.
- `assets/` holds `manifest.json` and the icon; its contents become the root of the plugin directory.
- `nix/package.nix` builds the plugin with nix-opendeck's `mkRustOpenDeckPlugin`: output `$out/opendeck-touchbar.sdPlugin/` (plus a `bin/` link for `--demo`), `passthru.pluginId`, and `passthru.udev` from `nix/udev-rules.nix`.
- `nix/opendeck.nix` wraps an `opendeck-core` patched with `nix/patches/opendeck-wide-keys.patch` with nix-opendeck's `wrapOpenDeck` (starterpack bundled, like upstream's default).
- This repo ships no NixOS or home-manager module. Users take `programs.opendeck` from `github:missmoode/nix-opendeck`, add `<plugin>.udev` to `services.udev.packages`, declare the `touchbar` group and turn `hardware.apple.touchBar.enable` off (see README.org).
