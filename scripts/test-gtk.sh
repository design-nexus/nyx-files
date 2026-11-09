#!/usr/bin/env bash
# Rust tests, the display-dependent ones included, on a private Xvfb display
# and D-Bus session, so nothing opens on the desktop. Arguments go to
# `cargo test --bin nyx-files`, for example a test name filter.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec env -u DISPLAY -u WAYLAND_DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE \
  GDK_BACKEND=x11 GTK_A11Y=none NO_AT_BRIDGE=1 STRATA_REQUIRE_GTK_TESTS=1 \
  xvfb-run -a -s "-screen 0 1280x1024x24 -nolisten tcp" \
  dbus-run-session -- cargo test --bin nyx-files "$@"
