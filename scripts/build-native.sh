#!/usr/bin/env bash
# Build the Rust/napi-rs cdylib and drop it into native/ under the platform name
# that native/index.js looks for. Large artifacts can be redirected off /data by
# setting CARGO_TARGET_DIR before invoking this script.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rust"

: "${CARGO_TARGET_DIR:=$ROOT/rust/target}"
export CARGO_TARGET_DIR

cargo build --release

PLAT="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ARCH="x64" ;;
  aarch64 | arm64) ARCH="arm64" ;;
esac
LIBC="gnu"
[ -f /etc/alpine-release ] && LIBC="musl"

case "$PLAT" in
  linux) NODE_NAME="spring-nav.linux-${ARCH}-${LIBC}.node" ;;
  darwin) NODE_NAME="spring-nav.darwin-${ARCH}.node" ;;
  mingw* | msys* | cygwin*) NODE_NAME="spring-nav.win32-${ARCH}-msvc.node" ;;
  *) NODE_NAME="spring-nav.node" ;;
esac

REL="$CARGO_TARGET_DIR/release"
SRC=""
for f in "$REL/libspring_nav.so" "$REL/libspring_nav.dylib" "$REL/spring_nav.dll"; do
  if [ -f "$f" ]; then SRC="$f"; break; fi
done
if [ -z "$SRC" ]; then
  echo "error: napi library not found under $REL" >&2
  exit 1
fi

mkdir -p "$ROOT/native"
cp "$SRC" "$ROOT/native/$NODE_NAME"
echo "Built $ROOT/native/$NODE_NAME"
