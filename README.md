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
```

The templates are compiled into the component, so a shell with no filesystem can
still print one, and the file a remedy names is the file that shipped rather than
a copy that drifted.

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
