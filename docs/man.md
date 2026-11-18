# NAME

arborist — what every repository in the fleet is missing, and what has rotted

# SYNOPSIS

`arborist report [--json] [--all] [--limit N] [--strict] [--fail-on LEVEL] < SNAPSHOT`

`arborist explain OWNER/NAME < SNAPSHOT`

`arborist ingest [--at RFC3339] < FORGE-PAGES`

`arborist checks [--json]`

`arborist template [readme|ci]`

# DESCRIPTION

A fleet rots in ways nobody notices one repository at a time: a licence that was
never added, a workflow that was written and then broke, a release nobody cut, a
description left empty, documentation whose links stopped resolving, a branch
that has not been touched in a year. Each is small. Together they are the reason
a thousand tokens go into a repository that cannot be released, and the reason
the next person cannot tell which of three hundred repositories is alive.

arborist asks nine questions of every repository and answers them from one
document. The questions are a fixed vocabulary (`arborist checks` prints it). The
thresholds — how stale is stale, how many branches are too many — are policy,
and the policy travels inside the snapshot so a report can be reproduced from
the bytes alone.

The answer has three values, not two. `pass` means the evidence says yes, `fail`
means it says no, and `unknown` means there was no evidence. Unknown is not a
pass: a repository whose CI state was never collected is not green, it is
unmeasured. `--strict` is a caller saying it would rather see the noise than the
lie.

# TWO COMMANDS, ONE PIPELINE

Collecting and scoring are separate on purpose.

```sh
./collector/collect.sh FuturePresentLabs mycelium-linux > snapshot.json
arborist report < snapshot.json
```

Collection needs a token and the network; scoring needs neither. That is why the
same binary runs inside `soil-command`, which links no host capability at all,
and inside a GitHub Action, where the token is the step's own. It is also why
two reports a week apart diff cleanly: nothing in the scoring path could have
changed between them except the fleet.

`arborist ingest` is the mapping from GitHub's GraphQL pages to a snapshot, and it
lives here rather than in a `jq` program so that a field the forge renames is a
failing test instead of a silently empty column.

# THE CHECKS

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

`arborist checks` prints the same table with the remedy for each, and `--json`
prints it for a tool.

# BADGES

A badge is a promise that something is being watched. The check reads every
image in the README and classifies it: a GitHub Actions status badge, a live CI
service (GitLab, CircleCI, Buildkite, Codecov), a service that has been shut
down, or something else entirely (crates, docs, licence) that is not CI.

It fails in three cases, in this order of how badly the README is lying:

1. a badge watches a service that no longer exists, so it renders as a broken
   image;
2. there is no CI badge at all;
3. an Actions badge names a workflow file that is not in `.github/workflows`,
   which is what happens when the workflow is deleted and the badge is not.

A repository with no README is `unknown` here rather than a second failure: the
`readme` check already owns that absence, and reporting one absence twice makes
a report harder to read, not more thorough.

# IN CI

The repository ships a GitHub action, so the check is four lines in a workflow:

```yaml
- uses: ajmwagar/arborist@v1
  with:
    orgs: OWNER                   # or omit for a pull-request gate on this repo
    fail-on: critical
```

`uses:` checks the action's own repository out, so the component is already
there and only wasmtime is installed. The report goes to the job summary and to
`arborist-report.json`.

`marbles: true` files one task per failing critical check, in the project of the
repository that failed it, through `mb` and the `soil:records` book — the same
command a person runs, so there is no second route into the tracker. Tasks are
titled `arborist: <check>` and labelled `arborist`; a re-run leaves existing
ones alone, closes the ones whose check now passes, and closes a task titled
after a repository rather than a check as superseded. On a runner without `mb`
the step fails rather than filing nothing.

# TEMPLATES

```sh
arborist template readme
arborist template ci
arborist template tracker
arborist template alerts
```

The files a repository needs to stop failing `readme`, `badges`, `ci` and
`tracker`.
They are compiled into the component, so they are reachable from a shell with no
filesystem, and the file a remedy names is the file that shipped.

Three of these ask for evidence a batched collection does not have. `ci-green`
and `docs-links` answer `unknown` until a collector fills `latest_run` and
`links`. `active` answers `unknown` until the snapshot carries `collected_at`.
Filling them is a collector change, not a arborist one.

# EXIT STATUS

- `0` — nothing at or above `--fail-on` failed.
- `1` — something did. The default is `critical`, so a missing licence or a red
default branch fails a pull request and a missing description does not.
- `2` — the input could not be read. A bad snapshot is never an empty fleet and
never a green one.

# EXAMPLES

Fail a pull request that adds a repository with no licence:

```sh
gh api graphql --paginate ... | arborist ingest > snapshot.json
arborist report --fail-on critical < snapshot.json
```

Ask why one repository is red:

```sh
arborist explain FuturePresentLabs/bob < snapshot.json
```

Gate on advisories in a nightly job, where a missing description should be
visible but should not page anybody:

```sh
arborist report --fail-on advisory --strict < snapshot.json
```

# READING THE REPORT

```
check            fail  unknown   share
license          119        -   ███████████████████       79%
ci                98        -   ████████████              65%
```

`fail` is repositories where the check said no. `unknown` is repositories where
it could not answer. `share` is `fail` over the repositories scored, so a
skipped repository (archived, or a fork) is counted in neither the numerator nor
the denominator.

# EXEMPTIONS

Some repositories do not need a check: a proprietary repository still needs a
question answered, but not a licence file. The snapshot carries that per
repository:

```json
{"org": "fpl", "name": "acme", "exempt": {"license": "proprietary, see legal"}}
```

An exemption is a decision somebody wrote down, so it is reported as `exempt`
with its reason rather than dropped. A check that quietly disappears is a check
the next person cannot audit.

# SEE ALSO

`docs/arborist.md`, `collector/fleet.graphql`, `mush doctor` (which checks the
shell, not the fleet).
