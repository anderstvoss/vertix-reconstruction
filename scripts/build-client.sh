#!/usr/bin/env sh
# Builds KrunkerRevival's browser client into client/dist, where
# config/server.toml points `client_dir`. Nothing it writes is committed.
#
#   scripts/build-client.sh [--source URL_OR_PATH] [--res-zip FILE]
#
# --source   KRP repository to clone: a URL, or the archive's mirror
#            (vertix-preservation/mirrors/KrunkerRevivalProject-vertix.git).
#            Default: the public KRP repository.
# --res-zip  A res.zip to serve instead of KRP's (for example the 2020
#            capture from the archive).
#
# Needs git, Node.js and pnpm. The commit is pinned so the client matches
# the server code ported from it (data/krp records the same commit).
set -eu

KRP_COMMIT=1e302cbc3015a648aeffb90517ec683ae19ed4df
SOURCE=https://github.com/KrunkerRevivalProject/vertix.git
RES_ZIP=

while [ $# -gt 0 ]; do
  case "$1" in
    --source) SOURCE=$2; shift 2 ;;
    --res-zip) RES_ZIP=$2; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CHECKOUT=$ROOT/client/krp
DIST=$ROOT/client/dist

for tool in git node pnpm; do
  command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required" >&2; exit 1; }
done

if [ ! -d "$CHECKOUT/.git" ]; then
  git clone --no-checkout "$SOURCE" "$CHECKOUT"
fi
git -C "$CHECKOUT" fetch --quiet "$SOURCE" "$KRP_COMMIT" 2>/dev/null || true
git -C "$CHECKOUT" checkout --quiet --detach "$KRP_COMMIT"

(cd "$CHECKOUT" && pnpm install --frozen-lockfile && pnpm --filter core build)

rm -rf "$DIST"
cp -R "$CHECKOUT/core/dist" "$DIST"
# Hats, shirts and sprays are loaded by path at run time, not bundled.
mkdir -p "$DIST/assets"
for d in hats shirts sprays; do
  cp -R "$CHECKOUT/core/assets/$d" "$DIST/assets/"
done
if [ -n "$RES_ZIP" ]; then
  cp "$RES_ZIP" "$DIST/res.zip"
fi
echo "client built from KRP $KRP_COMMIT into $DIST"
