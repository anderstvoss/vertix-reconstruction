#!/usr/bin/env sh
# Builds the Rust client (crates/client) for the browser into
# client/dist/rust/, where the server serves it at /rust/. Run it after
# scripts/build-client.sh, whose KRP checkout provides the font and whose
# client/dist holds the res.zip both clients load. Nothing it writes is
# committed.
#
#   scripts/build-rust-client.sh [--debug] [--font FILE]
#
# --debug  An unoptimised build (faster to compile).
# --font   font2.ttf to use instead of the one in client/krp.
#
# Needs the wasm32-unknown-unknown target (rustup target add
# wasm32-unknown-unknown). The desktop build is a plain
# `cargo run -p vertix_client --release` (it joins a server on this
# machine; `--server http://HOST:PORT` picks another).
set -eu

PROFILE=release
FONT=
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) PROFILE=debug; shift ;;
    --font) FONT=$2; shift 2 ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$ROOT/client/dist/rust
[ -n "$FONT" ] || FONT=$ROOT/client/krp/core/assets/font2.ttf
[ -f "$FONT" ] || { echo "font2.ttf not found at $FONT (run scripts/build-client.sh, or pass --font)" >&2; exit 1; }

if [ "$PROFILE" = release ]; then
  cargo build -p vertix_client --target wasm32-unknown-unknown --release --manifest-path "$ROOT/Cargo.toml"
else
  cargo build -p vertix_client --target wasm32-unknown-unknown --manifest-path "$ROOT/Cargo.toml"
fi

# The JavaScript glue ships with the crates; find the versions in use.
crate_dir() {
  cargo metadata --format-version 1 --manifest-path "$ROOT/Cargo.toml" --filter-platform wasm32-unknown-unknown |
    python3 -c "import json,sys,os; m=json.load(sys.stdin); print(next(os.path.dirname(p['manifest_path']) for p in m['packages'] if p['name']==sys.argv[1]))" "$1"
}
MINIQUAD=$(crate_dir miniquad)
JSUTILS=$(crate_dir sapp-jsutils)

mkdir -p "$OUT"
cat "$MINIQUAD/js/gl.js" "$JSUTILS/js/sapp_jsutils.js" "$ROOT/crates/client/web/vertix_net.js" > "$OUT/mq_js_bundle.js"
cp "$ROOT/target/wasm32-unknown-unknown/$PROFILE/vertix-client.wasm" "$OUT/"
cp "$ROOT/crates/client/web/index.html" "$OUT/"
cp "$FONT" "$OUT/font2.ttf"
echo "Rust client built into $OUT (open /rust/ on the server)"
