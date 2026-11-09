#!/bin/sh
set -eu

cd "$(dirname "$0")/../src-tauri"

sep=$(printf '\037')
flags="--remap-path-prefix=$HOME=~"
if [ -n "${CARGO_HOME:-}" ]; then
  flags="$flags$sep--remap-path-prefix=$CARGO_HOME=/cargo"
fi
CARGO_ENCODED_RUSTFLAGS="$flags"
export CARGO_ENCODED_RUSTFLAGS

cargo tauri build --bundles app "$@"

binary=target/release/chatly
if strings "$binary" | grep -qF "$HOME"; then
  echo "error: $binary still contains $HOME; do not publish it" >&2
  exit 1
fi
echo "Built target/release/bundle/macos/Chatly.app (no home-directory paths inside)"
