# arborist-ui

The arborist report as a server-rendered page, served at `arborist.myc`.

It is the same `arborist::Report` the command prints and the GPUI wall draws.
The page does no arithmetic the report has not already done, so a drift between
what CI gates on and what the page shows is not a bug that can be introduced.

## What it shows

| Path | What |
| --- | --- |
| `/` | headline counts, the distribution of every check, repositories worth opening first |
| `/checks` | every check: severity, fail/unknown/pass/exempt counts, and the remedy |
| `/repo?org=&name=` | one repository and every finding, with the detail |
| `/report.json` | the report the page rendered, as JSON |
| `/healthz` | `ok` |

## Where the data comes from

The Spore reads, in order:

1. `snapshot.json` — a snapshot from `arborist ingest`; evaluated in the guest
   with the same `evaluate` the command uses.
2. `report.json` — a report already produced by `arborist report --json`.
3. the fixture compiled into the module (`fixtures/fleet.json`) — the real
   fleet's shape with invented names, labelled `fixture` on the page.

A host mounts the directory holding the document with `--vfs-root`. When the
page says `fixture`, nothing was mounted, and its numbers are not the fleet's.

```sh
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/arborist_ui.wasm module.wasm
soil spore run . --surface http --listen 127.0.0.1:8092 \
  --vfs-root ~/.local/share/fungos/arborist-ui/data
```

## Two surfaces, one report

The GPUI wall in `../` is the frame surface: it draws the same report into a
texture. This Spore is the web surface: it answers HTTP with HTML. Both read the
arborist library; neither evaluates anything on its own.
