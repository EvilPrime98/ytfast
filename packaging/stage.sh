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
echo "==> staged at $OUT"
