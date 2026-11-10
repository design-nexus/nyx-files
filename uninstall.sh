#!/usr/bin/env bash
# Remove Nyx Files.
#
#   ./uninstall.sh            remove the program, its launcher entry and its integrations
#   ./uninstall.sh --purge    also remove its settings, history and cache
set -euo pipefail

APP_ID="io.github.design_nexus.nyx.Files"
say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf '\033[31m==>\033[0m %s\n' "$*" >&2; exit 1; }

purge=false
for a in "$@"; do
  case $a in
    files) ;;
    --purge) purge=true ;;
    *) die "This repository removes only files." ;;
  esac
done

bin="$HOME/.local/bin"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
desk="$share/applications"
services="$share/dbus-1/services"

# The file chooser and udiskie unlock integrations are set up from Files' settings.
if [[ -x $bin/nyx-files ]]; then
  "$bin/nyx-files" --uninstall-portal >/dev/null 2>&1 || true
  "$bin/nyx-files" --uninstall-udiskie-unlock >/dev/null 2>&1 || true
fi

rm -f "$bin/nyx-files" "$bin/nyx-files-rar" "$desk/$APP_ID.desktop" \
  "$share/icons/hicolor/scalable/apps/$APP_ID.svg" "$services/$APP_ID.FileManager1.service" \
  "$services/$APP_ID.service" "${XDG_DATA_HOME:-$HOME/.local/share}/nyx/actions/$APP_ID.json"
rm -rf "$share/licenses/nyx-files"
# Give "Open file location" back to the provider the installer set aside.
for backup in "$services"/*.service.nyx-files-backup; do
  if [[ -f $backup ]]; then mv -- "$backup" "${backup%.nyx-files-backup}"; fi
done
update-desktop-database "$desk" 2>/dev/null || true
if command -v xdg-mime >/dev/null && [[ -f /usr/share/applications/org.gnome.Nautilus.desktop ]]; then
  xdg-mime default org.gnome.Nautilus.desktop inode/directory
fi

if [[ $purge == true ]]; then
  rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/nyx-files" "${XDG_STATE_HOME:-$HOME/.local/state}/nyx-files" \
    "${XDG_CACHE_HOME:-$HOME/.cache}/nyx-files" "$share/nyx-files"
fi
say "Files removed."
