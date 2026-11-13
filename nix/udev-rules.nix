{
  writeText,
  coreutils,
}:

# The seat rules mirror the ones tiny-dfr ships, so the compositor keeps ignoring the Touch Bar.
writeText "70-opendeck-touchbar.rules" ''
  SUBSYSTEM=="drm", KERNEL=="card*", DRIVERS=="adp|appletbdrm", TAG-="master-of-seat", ENV{ID_SEAT}="seat-touchbar", GROUP="touchbar", MODE="0660"

  SUBSYSTEM=="input", ATTR{name}=="Apple Inc. Touch Bar Display Touchpad", ENV{ID_SEAT}="seat-touchbar"
  SUBSYSTEM=="input", ATTR{name}=="MacBookPro17,1 Touch Bar", ENV{ID_SEAT}="seat-touchbar"
  SUBSYSTEM=="input", ATTR{name}=="Mac14,7 Touch Bar", ENV{ID_SEAT}="seat-touchbar"
  SUBSYSTEM=="input", KERNEL=="event*", ATTRS{name}=="Apple Inc. Touch Bar Display Touchpad|MacBookPro17,1 Touch Bar|Mac14,7 Touch Bar", GROUP="touchbar", MODE="0660"

  ACTION=="add", SUBSYSTEM=="backlight", KERNEL=="228600000.dsi.0|228200000.display-pipe.0|appletb_backlight", RUN+="${coreutils}/bin/chgrp touchbar /sys%p/brightness", RUN+="${coreutils}/bin/chmod g+w /sys%p/brightness"
''
