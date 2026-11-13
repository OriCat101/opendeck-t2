# opendeck-touchbar

OpenDeck device plugin that turns the MacBook Touch Bar into a deck on Asahi Linux (NixOS, aarch64). One Rust binary owns the bar directly and replaces tiny-dfr: no function row, bar is blank when OpenDeck is not running. Target machine: MacBookPro17,1 (M1 13", j293).

## Status

- `--demo` (numbered keys, prints touches) verified on hardware as root with tiny-dfr stopped: display, orientation, key layout and touch mapping are correct without `OPENDECK_TOUCHBAR_FLIP`.
- Non-root works: with tiny-dfr stopped and the device nodes owned by the user, the demo and the plugin take the display without root (first opener becomes DRM master). Ad-hoc setup until reboot: `sudo chown ori /dev/dri/card2 /dev/input/event3 /sys/class/backlight/228600000.dsi.0/brightness`.
- The plugin connects to OpenDeck and registers its 16-key device (seen in the plugin log).
- Not yet tested: `nix/module.nix` (the permanent form of that access), backlight/idle dimming behaviour, wake from suspend.
- OpenDeck is not installed system-wide; `.#opendeck` builds `github:Kitt3120/opendeck-nix`'s package for aarch64. It must be built with this flake's nixpkgs, not opendeck-nix's pin: the webview aborts with `Could not create default EGL display: EGL_BAD_PARAMETER` when its glibc is older than the system Mesa's. The fixed-output hashes still match; the newer nixpkgs needs `libgit2` added to `buildInputs`.
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
- Backlight: `/sys/class/backlight/228600000.dsi.0/brightness`, 0..255, root-writable by default. The panel is OLED, hence dim after 30 s and off after 60 s without touch.
- The adp driver refuses opens from processes whose name starts with `X`.
- Both devices carry `ID_SEAT=seat-touchbar` from udev rules shipped with tiny-dfr; that keeps the compositor off them. `nix/module.nix` re-ships those rules (they vanish when tiny-dfr is disabled) and grants the `touchbar` group access.
- T2 Macs use a different stack (`appletbdrm`, `hid-appletb-*`); driver names are listed in the code but nothing there is tested.

## OpenDeck device plugin protocol (OpenDeck 2.14, `openaction` 2.8)

- OpenDeck launches the binary with `-port -pluginUUID -registerEvent -info`; `openaction::run(args)` handles the websocket. Handlers come from one `GlobalEventHandler` set with `set_global_event_handler` before `run`.
- `device_plugin::register_device(id, name, rows, columns, encoders, type)`; `type` is not interpreted (0). Keys are addressed by linear position. `key_down` / `key_up` / `unregister_device` take the device id.
- The first two characters of the device id must equal the manifest `DeviceNamespace` (`tb`; device id `tb-touchbar`).
- `device_plugin_set_image`: `image` is a `data:image/jpeg;base64` URL of a 144x144 key. `image: None` clears one key; `position: None` clears the whole device. `controller` is `Keypad`, `Encoder` or `Infobar`.
- `device_plugin_set_brightness`: percentage.
- Images can arrive immediately after registration, so the bar must be ready first.
- Manifest: `Actions: []`, `OS: [{Platform: linux}]`, `CodePathLin`, and `CodePaths` keyed by Rust target triple (`aarch64-unknown-linux-gnu`).
- Plugins live in `~/.config/opendeck/plugins/<dir>`; the directory name is the plugin UUID. OpenDeck `chmod 0755`s the binary before launch, so it must be a real user-owned file: copy it, do not symlink into the Nix store (`nix run .#install` does this).
- Plugin logs: `~/.local/share/opendeck/logs/plugins/opendeck-touchbar.sdPlugin.log`.
- Reference plugin on openaction 2.x: `ambiso/opendeck-akp05`. `4ndv/opendeck-akp03` is still on the incompatible 1.x API.

## Code map

- `src/bar/display.rs` DRM setup and rotated drawing; `src/bar/touch.rs` evdev slots to touch events; `src/bar/backlight.rs`; `src/bar/mod.rs` the `Bar` (keys, brightness, idle power) and `start()` which yields `KeyEvent`s.
- `src/layout.rs` key grid (`COLS = 16`, one row); `src/demo.rs` hardware test; `src/plugin.rs` OpenDeck glue.
- `nix/package.nix` builds the binary and the `.sdPlugin` directory; `nix/module.nix` is the NixOS module (`hardware.opendeckTouchBar.{enable,users}`), which asserts that `hardware.apple.touchBar.enable` is off.
