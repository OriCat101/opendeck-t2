{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.hardware.opendeckTouchBar;
in
{
  options.hardware.opendeckTouchBar = {
    enable = lib.mkEnableOption "user access to the Touch Bar for the OpenDeck Touch Bar plugin";

    users = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Users allowed to drive the Touch Bar (members of the `touchbar` group).";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !(config.hardware.apple.touchBar.enable or false);
        message = "hardware.opendeckTouchBar replaces tiny-dfr: set hardware.apple.touchBar.enable = false.";
      }
    ];

    users.groups.touchbar.members = cfg.users;

    # The seat rules mirror the ones tiny-dfr ships, so the compositor keeps ignoring the Touch Bar.
    services.udev.extraRules = ''
      SUBSYSTEM=="drm", KERNEL=="card*", DRIVERS=="adp|appletbdrm", TAG-="master-of-seat", ENV{ID_SEAT}="seat-touchbar", GROUP="touchbar", MODE="0660"

      SUBSYSTEM=="input", ATTR{name}=="Apple Inc. Touch Bar Display Touchpad", ENV{ID_SEAT}="seat-touchbar"
      SUBSYSTEM=="input", ATTR{name}=="MacBookPro17,1 Touch Bar", ENV{ID_SEAT}="seat-touchbar"
      SUBSYSTEM=="input", ATTR{name}=="Mac14,7 Touch Bar", ENV{ID_SEAT}="seat-touchbar"
      SUBSYSTEM=="input", KERNEL=="event*", ATTRS{name}=="Apple Inc. Touch Bar Display Touchpad|MacBookPro17,1 Touch Bar|Mac14,7 Touch Bar", GROUP="touchbar", MODE="0660"

      ACTION=="add", SUBSYSTEM=="backlight", KERNEL=="228600000.dsi.0|228200000.display-pipe.0|appletb_backlight", RUN+="${pkgs.coreutils}/bin/chgrp touchbar /sys%p/brightness", RUN+="${pkgs.coreutils}/bin/chmod g+w /sys%p/brightness"
    '';
  };
}
