# Arborist in Soil

This is a side-by-side port of the pure evaluator, not the production Spore yet.
The Rust component remains the oracle and the shipped command.

The first slice covers every check's pass/fail/unknown count:

```text
license ci ci-green release readme badges description docs-links tracker
metrics alerts active branches open-pulls stale-branches
```

It still emits a summary, not the full per-repository report document.

Run the conformance check (Rust report vs Soil counts):

```sh
SOIL_ROOT=~/src/ajmwagar-soil ./soil/check.sh
# soil evaluator matches Rust for 15 checks: ...
```

Run it directly against the Rust fixture with the bootstrapped Soil runtime:

```sh
WASM=$HOME/.local/share/soil/current/bin/soil-wasm.wasm
wasmtime run --dir . "$WASM" --prelude "$HOME/src/ajmwagar-soil/core/prelude.soil" \
  --call main "$HOME/src/ajmwagar-soil/core/json.soil" soil/arborist.soil \
  -- fixtures/fleet.json
```
