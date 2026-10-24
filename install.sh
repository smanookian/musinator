#!/bin/sh
# Install Musinator for the current user (no sudo).
#
#   curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh
#
# Run from a source checkout to build and install that instead.
# INSTALL_DIR changes where the binary goes (default ~/.local/bin).
set -eu

REPO="smanookian/musinator"
BIN_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

say() { printf '%s\n' "$*"; }
fail() { say "musinator install: $*" >&2; exit 1; }

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ARCH=x86_64 ;;
  *) fail "release builds are Linux x86_64 only; build from a source checkout instead" ;;
esac

# Runtime needs: PipeWire and libopus.
missing=""
for lib in libpipewire-0.3.so.0 libopus.so.0; do
  ldconfig -p 2>/dev/null | grep -q "$lib" || missing="$missing $lib"
done
if [ -n "$missing" ]; then
  say "Note: missing$missing. On Arch: sudo pacman -S pipewire opus"
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

HERE=$(cd "$(dirname "$0")" 2>/dev/null && pwd || true)
if [ -n "$HERE" ] && [ -f "$HERE/Cargo.toml" ] && grep -q '^name = "musinator"' "$HERE/Cargo.toml"; then
  say "Building from source in $HERE"
  command -v cargo >/dev/null || fail "cargo is needed to build from source"
  (cd "$HERE" && cargo build --release --locked -p musinator)
  cp "$HERE/target/release/musinator" "$TMP/musinator"
  DESKTOP="$HERE/packaging/musinator.desktop"
else
  URL="https://github.com/$REPO/releases/latest/download/musinator-$ARCH-linux.tar.gz"
  say "Downloading $URL"
  curl -fsSL "$URL" -o "$TMP/m.tar.gz" || fail "download failed"
  tar -xzf "$TMP/m.tar.gz" -C "$TMP"
  DESKTOP="$TMP/musinator.desktop"
fi

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 755 "$TMP/musinator" "$BIN_DIR/musinator"
[ -f "$DESKTOP" ] && install -m 644 "$DESKTOP" "$APP_DIR/musinator.desktop"

say "Installed $BIN_DIR/musinator"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "Add $BIN_DIR to your PATH." ;;
esac
say "Run: musinator"
