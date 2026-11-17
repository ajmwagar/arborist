# On a schedule, and on the bus

Two things turn arborist from something you run into something that tells you:

```text
   mycelium pulse            scan.sh                 unibus
   (cron, bounded)  ───────► collect → score ─────►  arborist.scan
                                   │
                                   └─► report.json   (kept for the view)
```

Neither half needs anything new inside the component. `arborist` is a `wasi:cli`
command with no socket, so it scores and the host transports — the same boundary
as every other integration in this thread.

## The scan

`collector/scan.sh` is the whole pass:

```sh
./collector/scan.sh FuturePresentLabs mycelium-linux
```

It collects, ingests, scores, writes the report to
`~/.local/state/arborist/report.json`, prints the headline, and publishes a
[digest](../src/digest.rs) when `UNIBUS_CLIENT` is set.

**The exit code is about the scan, not the fleet.** A red fleet is a successful
scan:

| exit | meaning |
|---|---|
| `0` | the audit ran; the verdict is in the event and the report |
| `1` | the audit ran and could not be published |
| `2` | the audit did not happen |

That split matters because a scheduler's run history is for "did the job run".
A nightly job that goes red every night while the fleet is unhealthy teaches
everyone to ignore red.

## The clock

`myceliumd` owns the scheduler; the Pulse Spore and `mycelium pulse` are clients.

```sh
mycelium pulse add \
  --id arborist.scan \
  --schedule "0 6 * * *" \
  --command "/Users/ajmwagar/code/arborist/collector/scan.sh FuturePresentLabs mycelium-linux" \
  --write
```

Two constraints from `myceliumd/src/pulse.rs`, both of which shaped the script:

- **`RUN_TIMEOUT` is 60 seconds.** `scan.sh` caps collection at `FLEET_DEADLINE`
  (default 50s) with `timeout(1)` and says so out loud when `timeout(1)` is
  missing, so a big fleet ends with a report rather than being killed with none.
- **The command is bounded at 8 KiB and is not a shell session.** It runs with
  the daemon's environment, so `ARBORIST_WASM`, `ARBORIST_REPORT` and
  `UNIBUS_CLIENT` must be set where the daemon runs, not in your shell.

## The bus

```sh
arborist digest < snapshot.json \
  | unibus-publish --config ~/.config/unibus/arborist.json --kind arborist.scan
```

`unibus-publish` is the one-shot client in `unibus/crates/unibus-sdk/src/bin`. It
exists because the embedded `Session` answers "queued", not "sent" — a CLI that
exits after queueing can drop the frame before the worker writes it. The
synchronous path also means the exit code is honest.

The published body is `arborist digest`: counts, the failing checks, the
repositories worth opening first, and one `headline` sentence. It is an object
under 8 KiB, which is what `Publish::validate` asks for and what a notification
can carry.

```json
{
  "orgs": ["FuturePresentLabs", "mycelium-linux"],
  "checked": 150,
  "critical": 133,
  "advisory": 17,
  "clean": 0,
  "unknown": 150,
  "failing": true,
  "headline": "133 of 150 repositories fail a critical check: license 119, ci 98, readme 26",
  "worst": [{ "repo": "FuturePresentLabs/outriggerapp.com", "checks": ["license", "ci"], "severity": "critical" }],
  "by_check": [{ "check": "license", "severity": "critical", "fail": 119, "unknown": 0 }]
}
```

A consumer can render that without asking arborist anything else: `failing` is
the verdict, `headline` is the notification, `worst` is what to open first.

## What is not here

`unibus-publish` needs a `ClientConfig` — installation, producer, hub endpoint,
zones, and a token file. Creating `arborist` as a producer with a grant that
allows `arborist.scan` is a router-side decision, and a token is a credential:
neither belongs in this repository.
