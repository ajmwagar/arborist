# Arborist in Soil

This is a side-by-side port of the pure evaluator, not the production Spore yet.
The Rust component remains the oracle and the shipped command.

The first slice covers the checks that read only top-level snapshot fields:

```text
license ci ci-green release readme description active branches open-pulls
```

It intentionally omits the evidence-heavy checks (badges, docs-links, tracker,
metrics, alerts, stale-branches) until their snapshot objects are ported.

Run the conformance check (Rust report vs Soil counts):

```sh
SOIL_ROOT=~/src/ajmwagar-soil ./soil/check.sh
# soil evaluator matches Rust for 9 checks: ...
```

Run it directly against the Rust fixture with the bootstrapped Soil runtime:

```sh
WASM=$HOME/.local/share/soil/current/bin/soil-wasm.wasm
wasmtime run --dir . "$WASM" --prelude "$HOME/src/ajmwagar-soil/core/prelude.soil" \
  --call main "$HOME/src/ajmwagar-soil/core/json.soil" soil/arborist.soil \
  -- fixtures/fleet.json
```
