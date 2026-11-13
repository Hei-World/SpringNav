#!/usr/bin/env bash
# Build the Rust/napi-rs cdylib and drop it into native/ under the platform name
# that native/index.js looks for. Large artifacts can be redirected off /data by
# setting CARGO_TARGET_DIR before invoking this script.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rust"

: "${CARGO_TARGET_DIR:=$ROOT/rust/target}"
export CARGO_TARGET_DIR

# Optional cross-target build: SN_TARGET=<rust triple> builds for that target
# instead of the host, so one machine can produce both arch binaries
# (e.g. Windows ARM64 building for x64 too).
TARGET="${SN_TARGET:-}"
if [ -n "$TARGET" ]; then
  cargo build --release --target "$TARGET"
  REL="$CARGO_TARGET_DIR/$TARGET/release"
else
  cargo build --release
  REL="$CARGO_TARGET_DIR/release"
fi

SRC=""
for f in "$REL/libspring_nav.so" "$REL/libspring_nav.dylib" "$REL/spring_nav.dll"; do
  if [ -f "$f" ]; then SRC="$f"; break; fi
done
if [ -z "$SRC" ]; then
  echo "error: napi library not found under $REL" >&2
  exit 1
fi

PLAT="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ARCH="x64" ;;
  aarch64 | arm64) ARCH="arm64" ;;
esac
LIBC="gnu"
[ -f /etc/alpine-release ] && LIBC="musl"

if [ -n "$TARGET" ]; then
  case "$TARGET" in
    x86_64-pc-windows-msvc)   NODE_NAME="spring-nav.win32-x64-msvc.node" ;;
    aarch64-pc-windows-msvc)  NODE_NAME="spring-nav.win32-arm64-msvc.node" ;;
    x86_64-apple-darwin)      NODE_NAME="spring-nav.darwin-x64.node" ;;
    aarch64-apple-darwin)     NODE_NAME="spring-nav.darwin-arm64.node" ;;
    x86_64-unknown-linux-gnu) NODE_NAME="spring-nav.linux-x64-gnu.node" ;;
    aarch64-unknown-linux-gnu) NODE_NAME="spring-nav.linux-arm64-gnu.node" ;;
    x86_64-unknown-linux-musl) NODE_NAME="spring-nav.linux-x64-musl.node" ;;
    *) NODE_NAME="spring-nav.node" ;;
  esac
else
  case "$PLAT" in
    linux) NODE_NAME="spring-nav.linux-${ARCH}-${LIBC}.node" ;;
    darwin) NODE_NAME="spring-nav.darwin-${ARCH}.node" ;;
    mingw* | msys* | cygwin*) NODE_NAME="spring-nav.win32-${ARCH}-msvc.node" ;;
    *) NODE_NAME="spring-nav.node" ;;
  esac
fi

mkdir -p "$ROOT/native"
cp "$SRC" "$ROOT/native/$NODE_NAME"
echo "Built $ROOT/native/$NODE_NAME"
