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

binaries=$(find target -path '*/release/bundle/macos/Chatly.app/Contents/MacOS/*' -type f 2> /dev/null || true)
if [ -z "$binaries" ]; then
  echo "error: no Chatly.app found under target/ to check; do not publish this build" >&2
  exit 1
fi
for binary in $binaries; do
  if ! strings "$binary" > /dev/null; then
    echo "error: couldn't read $binary; do not publish it" >&2
    exit 1
  fi
  if strings "$binary" | grep -qF "$HOME"; then
    echo "error: $binary still contains $HOME; do not publish it" >&2
    exit 1
  fi
  echo "Checked $binary: no home-directory paths inside"
done
