#!/usr/bin/env bash
# Install Nyx Files.
#
#   curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-files/main/install.sh | bash
#
# Files becomes the default app for folders and Trash links, and answers "Open file
# location" requests (FileManager1). It doesn't start or restart Files.
# NYX_BUILD=1 builds from source instead of fetching the release. NYX_SKIP_INTEGRATION=1
# leaves the default apps and other "Open file location" providers alone.
# ./uninstall.sh removes it.
set -euo pipefail

REPO="design-nexus/nyx-files"
APP_ID="io.github.design_nexus.nyx.Files"
say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31m==>\033[0m %s\n' "$*" >&2; exit 1; }

for a in "$@"; do
  case $a in
    # Files always opens folders; the suite's other options don't apply to it.
    files | --default | --gear) ;;
    -h | --help) sed -n '2,10p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "This repository installs only files." ;;
  esac
done

command -v hyprctl >/dev/null || warn "Hyprland wasn't found. Nyx apps are made for Omarchy (Hyprland), but run elsewhere too."

if command -v pacman >/dev/null; then
  need=()
  for p in gtk4 gtksourceview5 poppler-glib fontconfig gstreamer gst-plugins-base-libs bubblewrap pkgconf gcc git curl; do
    pacman -Qq "$p" >/dev/null 2>&1 || need+=("$p")
  done
  if ((${#need[@]})); then
    say "Installing ${need[*]}"
    sudo pacman -S --needed --noconfirm "${need[@]}"
  fi
fi

# A release archive, this checkout, the latest release, or a fresh clone.
here=""
[[ -n ${BASH_SOURCE[0]:-} && -f ${BASH_SOURCE[0]} ]] && here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
release_tag() {
  curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=100" 2>/dev/null |
    python3 -c 'import json,sys; print(next((r["tag_name"] for r in json.load(sys.stdin) if not r["draft"] and not r["prerelease"]), ""))'
}
prebuilt=false
if [[ -n $here && -f $here/.prebuilt ]]; then
  src=$here prebuilt=true
elif [[ -n $here && -f $here/Cargo.toml && -d $here/rar-helper ]]; then
  src=$here
elif [[ ${NYX_BUILD:-} != 1 ]] && tag=$(release_tag) && [[ -n $tag ]]; then
  say "Fetching Files $tag"
  src="$work/src"
  mkdir -p "$src"
  curl -fsSL "https://github.com/$REPO/releases/download/$tag/files-x86_64.tar.gz" | tar -xz -C "$src"
  prebuilt=true
else
  say "Fetching the source"
  git clone -q --depth 1 "https://github.com/$REPO.git" "$work/src"
  src="$work/src"
fi

if [[ $prebuilt == false ]]; then
  if ! command -v cargo >/dev/null; then
    command -v pacman >/dev/null || die "Rust (cargo) is needed to build Files. Install it and run this again."
    say "Installing rust"
    sudo pacman -S --needed --noconfirm rust
  fi
  say "Building Files (a few minutes the first time)"
  (cd "$src" && cargo build --release --locked -p nyx-files -p nyx-files-rar)
  target="${CARGO_TARGET_DIR:-$src/target}/release"
else
  target="$src/target/release"
fi

say "Installing Files"
bin="$HOME/.local/bin"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
desk="$share/applications"
services="$share/dbus-1/services"
mkdir -p "$bin" "$desk" "$services"
# Replace the files rather than writing into them, so a running Files keeps its old
# binary until it restarts.
for b in nyx-files nyx-files-rar; do
  install -m 755 "$target/$b" "$bin/$b.new" && mv -f "$bin/$b.new" "$bin/$b"
done
sed "s|^Exec=nyx-files|Exec=$bin/nyx-files|" "$src/data/$APP_ID.desktop" >"$desk/$APP_ID.desktop"
install -Dm644 "$src/data/icons/scalable/apps/$APP_ID.svg" "$share/icons/hicolor/scalable/apps/$APP_ID.svg"

# "Open file location" (FileManager1): another per-user provider is set aside;
# ./uninstall.sh puts it back.
integrate=true
[[ ${NYX_SKIP_INTEGRATION:-0} == 1 ]] && integrate=false
for service in "$services"/*.service; do
  [[ $integrate == true && -f $service && $service != "$services/$APP_ID.FileManager1.service" ]] || continue
  if grep -qx 'Name=org.freedesktop.FileManager1' "$service"; then
    mv -- "$service" "$service.nyx-files-backup"
  fi
done
sed "s|^Exec=/usr/bin/nyx-files|Exec=$bin/nyx-files|" "$src/data/$APP_ID.FileManager1.service" >"$services/$APP_ID.FileManager1.service"

# Files is GPL-3.0-or-later; the RAR helper keeps its own MIT/UnRAR notices.
install -d "$share/licenses/nyx-files"
for notice in LICENSE LICENSE-MIT COPYING THIRD_PARTY_LICENSES.md; do
  install -m 644 "$src/$notice" "$share/licenses/nyx-files/$notice"
done

update-desktop-database "$desk" 2>/dev/null || true
gtk-update-icon-cache -f -t -q "$share/icons/hicolor" 2>/dev/null || warn "Could not refresh the application icon cache."
if [[ $integrate == false ]]; then
  :
elif command -v xdg-mime >/dev/null; then
  xdg-mime default "$APP_ID.desktop" inode/directory x-scheme-handler/trash
else
  warn "xdg-mime wasn't found, so Files isn't the default app for folders."
fi

case ":$PATH:" in
  *":$bin:"*) ;;
  *) warn "$bin isn't on your PATH; open Files from the app launcher, or add it to PATH." ;;
esac
say "Done. Open Files from the app launcher."
