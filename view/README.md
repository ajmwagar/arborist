# arborist-view

The fleet as one frame. A GPUI Spore: it owns the view, the host owns the WebGPU
device and the compositor.

```sh
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/arborist_view.wasm module.wasm
soil spore run . --surface frame --frame /tmp/arborist.png --size 1280x800
```

`module.wasm` is built, not committed — 9.7 MB of GPUI and WebGPU that nothing
consumes without a host that can present it.

![the wall](preview/wall.png)

## What it draws

The same `arborist::Report` the command prints, rendered once:

- the headline sentence, which is the whole point of the tool;
- four numbers: failing critical, advisory only, clean, and unmeasured;
- every check with a bar, because the shape of the distribution is the argument;
- the repositories worth opening first, with their failing checks as chips.

It computes nothing. There is no second evaluator here — a wall that did its own
arithmetic would eventually disagree with the gate, and the wall is the thing
people believe.

## Where the data comes from

`fixtures/fleet.json` is compiled in: a snapshot with the real fleet's shape and
invented names, because this repository is public and the fleet is not.

Fetching a live report over `soil:http` is the next step and is a change to where
`SNAPSHOT` is read, not to the view.
