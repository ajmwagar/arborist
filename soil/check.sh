#!/bin/sh
# Compare the Soil evaluator with the Rust oracle on the checked-in fleet.
#
#   SOIL_ROOT=~/src/ajmwagar-soil ./soil/check.sh
#
# The first slice covers nine top-level checks; the comparison is intentionally
# limited to those so a missing nested snapshot type cannot hide a real mismatch.
set -eu
cd "$(dirname "$0")/.."

SOIL_ROOT=${SOIL_ROOT:-$HOME/src/ajmwagar-soil}
SOIL_WASM=${SOIL_WASM:-$HOME/.local/share/soil/current/bin/soil-wasm.wasm}
WASMTIME=${WASMTIME:-wasmtime}
OUT=${TMPDIR:-/tmp}/arborist-soil-report.json
RUST=${TMPDIR:-/tmp}/arborist-rust-report.json

[ -f "$SOIL_WASM" ] || { echo "no Soil runtime at $SOIL_WASM" >&2; exit 2; }
[ -f "$SOIL_ROOT/core/prelude.soil" ] || { echo "no Soil source at $SOIL_ROOT" >&2; exit 2; }

"$WASMTIME" run --dir "$SOIL_ROOT" --dir . "$SOIL_WASM" \
  --prelude "$SOIL_ROOT/core/prelude.soil" --call main \
  "$SOIL_ROOT/core/json.soil" "$SOIL_ROOT/core/date.soil" \
  "$SOIL_ROOT/std/wasi/fs.soil" soil/arborist.soil \
  -- fixtures/fleet.json > "$OUT"

cargo run -q --features cli --bin arborist -- report --json --fail-on none \
  < fixtures/fleet.json > "$RUST"

python3 - "$OUT" "$RUST" <<'PY'
import json, sys
soil = json.load(open(sys.argv[1]))
rust = json.load(open(sys.argv[2]))
checked = [row["check"] for row in soil["summary"]["by_check"]]
by_rust = {row["check"]: row for row in rust["summary"]["by_check"]}
bad = []
for row in soil["summary"]["by_check"]:
    if row != by_rust[row["check"]]:
        bad.append((row["check"], row, by_rust[row["check"]]))
for s, r in [("checked", soil["summary"]["checked"]), ("skipped", soil["summary"]["skipped"])]:
    if r != rust["summary"][s]:
        bad.append((s, r, rust["summary"][s]))
if soil["orgs"] != rust["orgs"]:
    bad.append(("orgs", soil["orgs"], rust["orgs"]))
if bad:
    for item in bad:
        print("DIFF", item)
    sys.exit(1)
print(f"soil evaluator matches Rust for {len(checked)} checks: {', '.join(checked)}")
PY
