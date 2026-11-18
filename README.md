# arborist

[![ci](https://github.com/ajmwagar/arborist/actions/workflows/ci.yml/badge.svg)](https://github.com/ajmwagar/arborist/actions/workflows/ci.yml)

Fleet health for source code: what every repository is missing, and what has rotted.

A fleet does not rot all at once. It rots one small thing at a time — a licence
that was never added, a workflow written and then broken, a release nobody cut, a
description left empty, documentation whose links stopped resolving, a branch
nobody has touched in a year. Each is small. Together they are the reason a
thousand tokens go into a repository that cannot be released, and the reason
nobody can tell which of three hundred repositories is actually alive.

`arborist` asks nine questions of every repository and answers them from one
document.

```text
forge            snapshot            report
(GraphQL) ──ingest──► (repos) ──evaluate──► (findings) ──render──► text | JSON
```

One function in the middle, and it is pure: `evaluate` reads no clock, opens no
socket and touches no path. The answer a shell prints, the answer CI gates on,
and the answer a view draws are the same answer from the same bytes. That is the
whole reason a check is written once instead of twice.

## The checks

| check | severity | question |
|---|---|---|
| `license` | critical | does the repository declare a licence |
| `ci` | critical | is there a workflow under `.github/workflows` |
| `ci-green` | critical | did the last run on the default branch pass |
| `release` | advisory | has it ever cut a release |
| `readme` | critical | is there a README a stranger could start from |
| `badges` | advisory | do its badges point at something that still exists |
| `description` | advisory | does the forge description say what it is |
| `docs-links` | advisory | do the documentation links resolve |
| `tracker` | critical | do the agent instructions name marbles rather than beads |
| `metrics` | advisory | does the service it runs export Prometheus metrics |
| `alerts` | advisory | does it define alert rules for the metrics it exports |
| `active` | advisory | was it pushed to inside the staleness window |
| `branches` | advisory | are branches under the limit |
| `open-pulls` | advisory | are open pull requests under the limit |

`arborist checks` prints the same table with the remedy for each.

## Unknown is not a pass

Every check answers `pass`, `fail`, `unknown` or `exempt`. A repository whose CI
state was never collected is not green, it is unmeasured, and the summary counts
it separately. `--strict` is a caller saying it would rather see the noise than
the lie.

```text
check            fail  unknown   share
license          119        -   ███████████████████       79%
ci                98        -   ████████████              65%
ci-green           0      150                              0%
release          145        -   ███████████████████████   97%
readme            26        -   ████                      17%
badges           135       13   ██████████████████████    90%
docs-links         0      150                              0%
tracker           36        -   ██████                    24%
metrics           34        -   ██████                    23%
alerts             5        -   █                          3%
active            46        -   ███████                   31%
branches           4        -   █                          3%

  unknown is not a pass: the collector did not supply that evidence.
  `--strict` fails on it.
```

Those numbers are one real run over a 150-repository fleet (42 more were skipped
as archived or forks). 79% of repositories had no licence a forge could name, 65%
had no workflow, 97% had never cut a release, and 90% had a README with no status
badge on it.

## The badges check

A badge is a promise that something is being watched. A badge pointing at a
workflow file that was deleted six months ago is not a promise, it is a broken
image — and it is the most common way a README starts lying, because nothing in
the repository notices when the workflow it advertises goes away.

So `badges` reads the README's images and classifies each one. It fails when a
badge watches a service that no longer exists (`travis-ci.org`), when there is no
CI badge at all, and when an Actions badge names a workflow file that is not in
`.github/workflows`. Everything else — crates, docs, licence, coverage — is
ignored, because none of it is CI.

## Fix it, not just find it

Every remedy names a file you can have:

```sh
arborist template readme   # a README whose badge points at a workflow that exists
arborist template ci       # the workflow that badge points at
arborist template tracker  # the marbles block that replaces the one naming beads
arborist template alerts   # alert rules, where the alerts check reads them
```

The templates are compiled into the component, so a shell with no filesystem can
still print one, and the file a remedy names is the file that shipped rather than
a copy that drifted.

## Install it as a check

The action is the same component, so a pull request and the nightly audit never
disagree about a repository.

```yaml
# Gate a pull request on the repository it is in.
- uses: ajmwagar/arborist@v1
  with:
    fail-on: critical
```

```yaml
# Audit whole owners on a schedule.
- uses: ajmwagar/arborist@v1
  with:
    orgs: FuturePresentLabs mycelium-linux
    fail-on: advisory
    strict: true
```

The report is written to the job summary and to `arborist-report.json`; the step
fails with the gate's exit code, so it is a real check and not a decoration.

`uses:` checks this repository out for you, so `bin/arborist.wasm` is already
there — the action installs wasmtime and runs it. No release asset, no build
step, no cache to keep warm.

### Filing the findings as Marbles tasks

Opt in and each failing critical check becomes one task, in the project of the
repository that failed it:

```yaml
- uses: ajmwagar/arborist@v1
  with:
    orgs: FuturePresentLabs
    marbles: true
    marbles-limit: 25
```

A repository is resolved to the project for its `OWNER/NAME` when the registry
has one, else its bare name when that is unambiguous, else `OWNER/NAME`, which
`mb` creates on first use. A task in one central project says "a repository
somewhere is missing a licence"; nobody owns it and nothing runs `mb ready` on
it. A task in the repository's own project is in front of the person who can fix
it.

It reconciles rather than appends: a task that already exists is left alone, a
task whose check now passes is closed, and a task titled after a repository
rather than a check — the shape this used to file — is closed as superseded. So
a nightly job does not produce a new backlog every night. `marbles-limit` exists
because a first audit of a large fleet fails hundreds of checks at once, and
filing them all buries the backlog it was meant to create.

It talks to the tracker through `mb` — the Marbles command over `soil:records`
that every host in the fleet already has — rather than an HTTP client or a
service token. A GitHub-hosted runner has no records book, so on one of those the
step **fails** rather than quietly filing nothing. Use a self-hosted runner, or
leave `marbles` off.

## Use it

```sh
./collector/collect.sh OWNER... > snapshot.json   # needs gh and a token
arborist report < snapshot.json
arborist explain OWNER/NAME < snapshot.json
```

`--fail-on critical` (the default) exits non-zero when a repository has no licence,
no CI, or a red default branch, which is the shape that belongs in a pull request
gate. `--fail-on advisory --strict` belongs in a nightly job, where a missing
description should be visible but should not page anybody.

The collector is a reference, not the only one. It exists to show that the
snapshot contract is writable by anything: a shell here, a scheduled job, a
service that already holds a token. Collection needs the network; scoring needs
neither, which is why the same binary runs in a sandbox, in CI, and on a laptop
whose only credential is the caller's own.

## Two documents and a function

```text
src/check.rs      the vocabulary: nine questions and their severities
src/snapshot.rs   the fact sheet a collector produces
src/policy.rs     the thresholds, and evaluate()
src/report.rs     the verdicts, the rollup, and the terminal rendering
src/ingest.rs     GitHub GraphQL pages -> snapshot
src/date.rs       RFC 3339 in, whole days out
src/bin/arborist.rs  the `wasi:cli` command
collector/        one GraphQL query and nine lines of shell
tests/            the document contract, from forge bytes to report JSON
```

The mapping from GraphQL to a snapshot is Rust with a test per shape the forge
returns, not forty fields of `jq`. A renamed field there is a failing test, not a
silently empty column.

Policy travels inside the snapshot, so a report is reproducible from its bytes
alone. An exemption is reported as `exempt` with its reason rather than dropped —
a check that quietly disappears is one the next person cannot audit.

## On a schedule, and on the bus

```sh
./collector/scan.sh FuturePresentLabs mycelium-linux     # one whole pass
mycelium pulse add --id arborist.scan --schedule "0 6 * * *" \
  --command "/path/to/arborist/collector/scan.sh FuturePresentLabs mycelium-linux" --write
```

`scan.sh` scores, keeps the report, and publishes two shapes to unibus when
`UNIBUS_CLIENT` points at a client config:

* `arborist.scan` — the digest: counts, the failing checks, the repositories
  worth opening first, and one headline sentence. One message a person or a
  dashboard reads whole.
* `arborist.finding` — one message per repository that fails something, from
  `arborist events`. Level-triggered, so it says what is failing now rather than
  what changed; a subscriber that was down for the scan still learns the truth
  on the next one.

The exit code is about the scan, not the fleet: a red fleet is a successful
scan, and a scheduler whose runs go red every night teaches everyone to ignore
red. See [`docs/schedule-and-bus.md`](docs/schedule-and-bus.md).

## Build

```sh
cargo test --features cli
cargo build --release --target wasm32-wasip2 --features cli --bin arborist
cp target/wasm32-wasip2/release/arborist.wasm bin/arborist.wasm
```

`bin/arborist.wasm` is a `wasi:cli` component and `spore.toml` declares it as the
package's one command, so a host that installs the package puts `arborist` on the
shell's PATH. It imports nothing but WASI: no host capability is linked, which is
what lets it run in a sandbox that grants nothing at all.

## Licence

MIT.

---

For granddad.
