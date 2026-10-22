#!/usr/bin/env bash
# Builds LAME 3.100 (LGPL-2.0-or-later) as a shared library for the MP3 export (EXP-005,
# LIC-006) on macOS and Linux. scripts/build-lame.ps1 is the same for Windows.
#
# LAME is not linked into Xirius (MIT OR Apache-2.0) and is not a cargo dependency: the app
# loads this library at run time, and it is shipped next to the executable with its licence
# and where its source came from (see assets/lame/README.md).
#
# What it does:
#   1. Downloads lame-3.100.tar.gz from SourceForge (the official release) and checks its SHA-256.
#   2. Builds the library alone (no command-line lame) with LAME's own configure and make.
#        macOS: libmp3lame.dylib, one file for arm64 and x86_64 (lipo), macOS 11 and later.
#        Linux: libmp3lame.so for this machine's architecture.
#   3. Copies it with LAME's COPYING to target/lame/<os>/, and next to the executables in
#      target/debug and target/release, where the app and the tests look for it.
#
# Usage (from anywhere):
#   scripts/build-lame.sh               # build, then copy into target/debug and target/release
#   scripts/build-lame.sh --no-install  # build only
#
# Needs: a C compiler, make, curl and tar (macOS: the Xcode Command Line Tools).
# Linux distributions ship libmp3lame.so.0 as well, which the app finds by itself.

set -euo pipefail

VERSION=3.100
URL="https://downloads.sourceforge.net/project/lame/lame/$VERSION/lame-$VERSION.tar.gz"
SHA256=ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e

INSTALL=1
[ "${1:-}" = "--no-install" ] && INSTALL=0

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=${WORK:-$ROOT/target/lame}
case "$(uname -s)" in
    Darwin)
        FILE=libmp3lame.dylib
        ARCHS="arm64 x86_64"
        OUT=$WORK/macos
        ;;
    Linux)
        FILE=libmp3lame.so
        ARCHS=$(uname -m)
        OUT=$WORK/linux-$ARCHS
        ;;
    *)
        echo "build-lame.sh is for macOS and Linux; on Windows use scripts/build-lame.ps1" >&2
        exit 1
        ;;
esac
mkdir -p "$WORK" "$OUT"

# 1. Source
TARBALL=$WORK/lame-$VERSION.tar.gz
if [ ! -f "$TARBALL" ]; then
    echo "downloading $URL"
    curl -sSL --fail -o "$TARBALL" "$URL"
fi
if command -v sha256sum >/dev/null; then
    GOT=$(sha256sum "$TARBALL" | cut -d' ' -f1)
else
    GOT=$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)
fi
if [ "$GOT" != "$SHA256" ]; then
    rm -f "$TARBALL"
    echo "SHA-256 mismatch for lame-$VERSION.tar.gz: got $GOT, want $SHA256" >&2
    exit 1
fi

# 2. One build per architecture
JOBS=${JOBS:-4}
BUILT=()
for ARCH in $ARCHS; do
    SRC=$WORK/src-$(uname -s)-$ARCH
    rm -rf "$SRC"
    mkdir -p "$SRC"
    tar -xzf "$TARBALL" -C "$SRC" --strip-components=1
    # 3.100 lists lame_init_old among the library's symbols but no longer defines it; Apple's
    # linker refuses that (Homebrew's formula drops the line too). The source is not changed.
    sed '/lame_init_old/d' "$SRC/include/libmp3lame.sym" > "$SRC/include/libmp3lame.sym.new"
    mv "$SRC/include/libmp3lame.sym.new" "$SRC/include/libmp3lame.sym"
    CONFIGURE=(--prefix="$SRC/prefix" --enable-shared --disable-static --disable-frontend
        --disable-dependency-tracking --disable-debug)
    if [ "$(uname -s)" = Darwin ]; then
        HOST=$ARCH
        [ "$ARCH" = arm64 ] && HOST=aarch64
        FLAGS="-arch $ARCH -mmacosx-version-min=11.0"
        (cd "$SRC" && CFLAGS="-O2 $FLAGS" LDFLAGS="$FLAGS" \
            ./configure "${CONFIGURE[@]}" --host="$HOST-apple-darwin")
    else
        (cd "$SRC" && CFLAGS="-O2" ./configure "${CONFIGURE[@]}")
    fi
    make -C "$SRC" -j"$JOBS"
    make -C "$SRC" install
    if [ "$(uname -s)" = Darwin ]; then
        BUILT+=("$SRC/prefix/lib/libmp3lame.0.dylib")
    else
        BUILT+=("$SRC/prefix/lib/libmp3lame.so.0")
    fi
done

if [ "$(uname -s)" = Darwin ]; then
    lipo -create "${BUILT[@]}" -output "$OUT/$FILE"
    install_name_tool -id "@rpath/$FILE" "$OUT/$FILE"
else
    cp -L "${BUILT[0]}" "$OUT/$FILE"
fi

# 3. Licence and where the source came from, next to the library.
cp "$SRC/COPYING" "$OUT/LAME-COPYING.txt"
cat > "$OUT/LAME-SOURCE.txt" <<EOF
LAME $VERSION, built by scripts/build-lame.sh from the official release:
  $URL
  SHA-256 $SHA256
Licence: GNU Library General Public License 2.0 or later (LAME-COPYING.txt).
Not modified. Replace $FILE with another build of the same name to use that instead.
EOF
echo "built $OUT/$FILE"

if [ "$INSTALL" = 1 ]; then
    for PROFILE in debug release; do
        DIR=$ROOT/target/$PROFILE
        mkdir -p "$DIR"
        cp "$OUT/$FILE" "$OUT/LAME-COPYING.txt" "$OUT/LAME-SOURCE.txt" "$DIR/"
        echo "copied to $DIR"
    done
fi
