#!/usr/bin/env bash
# Assemble the app folder `dist/YTFast/` (what the installer / tarball ships).
#
#   packaging/stage.sh
#
# Layout:
#   ytfast(.exe)         the app (fonts and icons are embedded)
#   LICENSE, NOTICE.txt
#
# yt-dlp is NOT bundled: users install it themselves (or point Settings at it).
set -euo pipefail

MPV_RELEASE=20261105
MPV_ARCHIVE=mpv-dev-x86_64-20261105-git-3138fc8eea.7z
MPV_SHA256=f463cc69a7a1610937688b837be9f005cf163c26f2e35e50aec8cc7bde2894b4

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$HERE/dist/YTFast"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) EXE=.exe ;;
  Linux)                EXE= ;;
  *) echo "unsupported platform $(uname -s)" >&2; exit 1 ;;
esac

echo "==> cargo build --release"
(cd "$HERE" && cargo build --release --locked)

rm -rf "$OUT"
mkdir -p "$OUT"
cp "$HERE/target/release/ytfast$EXE" "$OUT/"
cp "$HERE/packaging/NOTICE.txt" "$OUT/NOTICE.txt"
[ -f "$HERE/LICENSE" ] && cp "$HERE/LICENSE" "$OUT/LICENSE" || true
if [ -n "$EXE" ]; then
  CACHE="$HERE/target/libmpv"
  mkdir -p "$CACHE"
  if [ ! -f "$CACHE/$MPV_ARCHIVE" ]; then
    echo "==> downloading libmpv $MPV_RELEASE"
    curl -fL --retry 3 -o "$CACHE/$MPV_ARCHIVE"       "https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/$MPV_RELEASE/$MPV_ARCHIVE"
  fi
  echo "$MPV_SHA256  $CACHE/$MPV_ARCHIVE" | sha256sum -c -     || { rm -f "$CACHE/$MPV_ARCHIVE"; echo "libmpv archive failed its checksum" >&2; exit 1; }
  if command -v 7z >/dev/null 2>&1; then
    7z e -y -o"$CACHE" "$CACHE/$MPV_ARCHIVE" libmpv-2.dll >/dev/null
  else
    tar -xf "$CACHE/$MPV_ARCHIVE" -C "$CACHE" libmpv-2.dll
  fi
  cp "$CACHE/libmpv-2.dll" "$OUT/"
fi
echo "==> staged at $OUT"
