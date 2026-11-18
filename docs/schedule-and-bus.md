# On a schedule, and on the bus

Two things turn arborist from something you run into something that tells you:

```text
   mycelium pulse            scan.sh                 unibus
   (cron, bounded)  ───────► collect → score ─────►  arborist.scan
                                   │                 arborist.finding
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
`~/.local/state/arborist/report.json`, prints the headline, and publishes when
`UNIBUS_CLIENT` is set.

## What goes on the bus

Two kinds, because the bus serves two kinds of reader:

| kind | body | for |
|---|---|---|
| `arborist.scan` | the [digest](../src/digest.rs): counts, failing checks, the ten repositories to open first, one headline | a person, a dashboard, a notification |
| `arborist.finding` | one [event](../src/events.rs) per repository that fails something: its failing checks with details, and the checks it has no evidence for | a subscriber that owns one repository |

`arborist events` renders the second as one JSON object per line, which is what
`unibus-publish` takes and what a `while read` loop can publish without a
parser. The events are **level-triggered**: an event says what is failing now,
not what changed since the last scan, so a subscriber that was down still learns
the truth on the next one. Working out the transition is the subscriber's
business, and the report file is there for it.

A repository that fails nothing is not an event. Neither is one whose only
unknown checks are missing evidence — "the collector did not say" is not
something a person can fix by opening the repository.

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
mycelium pulse add --id arborist.scan --schedule "0 6 * * *" --write \
  --command 'PATH=/Users/ajmwagar/.local/bin:$PATH \
    ARBORIST_WASM=/Users/ajmwagar/code/arborist/bin/arborist.wasm \
    /Users/ajmwagar/code/arborist/collector/scan.sh FuturePresentLabs mycelium-linux'
```

**The daemon schedules in UTC.** `0 6 * * *` fires at 06:00 UTC — 22:00 the
previous evening in Pacific time. `mycelium pulse list` shows `NEXT_AT_MS` as a
Unix timestamp, which is the only way to be sure; convert it before trusting a
schedule.

The command runs through `sh -c`, so the `VAR=value` prefixes and `$PATH` are
expanded by the daemon's shell, not by yours. Its stdout and stderr become the
pulse run's `detail`, which is why the headline line is the last thing printed:
`mycelium pulse runs arborist.scan` is where you read what the fleet said.

Two constraints from `myceliumd/src/pulse.rs`, both of which shaped the script:

- **`RUN_TIMEOUT` is 60 seconds.** `scan.sh` caps collection at `FLEET_DEADLINE`
  (default 50s) with `timeout(1)` and says so out loud when `timeout(1)` is
  missing, so a big fleet ends with a report rather than being killed with none.
  A measured two-owner run (192 repositories, 13 GraphQL pages, wasm component)
  takes **42 seconds**, which leaves less headroom than it looks: at that rate a
  slower API day hits the 60-second wall before the deadline helps. Two owners is
  the honest limit of one pulse today.
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

## Making the producer real

`unibus-publish` needs a `ClientConfig`, and the router needs to know the
producer. Four things, all of them refusals the router will tell you about one at
a time — this is what getting it wrong looks like, in order:

1. **A token.** 32+ characters, `0600`, at a path the node entry names. The
   router compares it constant-time against the file.
2. **A capability.** The router refuses to start at all with
   `node arborist has no capabilities`. A publisher still needs one; it is what
   a message can be routed *to*. `arborist.publisher` is the name used here.
3. **`publish_zones`**, not just `zones`. `zones` is where the node *is*;
   `publish_zones` is what it may *target*. Without it a publication is refused
   with `may not target zone default`.
4. **A route.** Without one: `no route for kind 'arborist.scan'`. With one
   pointing at a capability nobody holds: `matched no live node` — the frame is
   accepted and dropped, which is a bus with no subscriber rather than a
   mistake.

The node entry:

```json
{
  "producer_id": "arborist",
  "token_file": "/Users/you/.config/unibus/tokens/arborist.token",
  "zones": ["default"],
  "capabilities": ["arborist.publisher"],
  "publishes": ["arborist.scan"],
  "publish_zones": ["default"]
}
```

and the route:

```json
{ "kind": "arborist.scan", "target": { "mode": "capability", "name": "arborist.reader" } }
```

Nothing holds `arborist.reader` yet, so a scan is published, accepted, and
delivered to nobody. That is the correct behaviour for a bus with no subscriber,
and it is one capability declaration away from having one: whatever should read
fleet health declares `arborist.reader` and starts receiving scans.

The router reads its config **once, at startup** — there is no reload and
`unibusctl` is diagnostics only, so a new node goes live on the next restart.
