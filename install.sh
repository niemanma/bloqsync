#!/usr/bin/env bash
# bloqsync – source build helper (installs deps and builds the GUI).
# For most users, prefer the prebuilt .deb / .rpm / AppImage from Releases.
set -euo pipefail
echo ">>> installing build dependencies (needs sudo)"
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libpipewire-0.3-dev libspa-0.2-dev \
  libpulse-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf
echo ">>> installing rust (if missing)"
command -v cargo >/dev/null 2>&1 || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env" 2>/dev/null || true
echo ">>> installing udev rule"
sudo cp contrib/99-bloqsync.rules /lib/udev/rules.d/99-bloqsync.rules
sudo udevadm control --reload-rules || true
echo ">>> building"
cd gui
cargo tauri build --bundles deb
echo ">>> done. Package in gui/target/release/bundle/deb/ ; run: sudo apt install ./gui/target/release/bundle/deb/*.deb"
