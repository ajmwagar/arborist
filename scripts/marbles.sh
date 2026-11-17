#!/bin/sh
# marbles.sh: turn an arborist report into tracker work, through `mb`.
#
#   arborist report --json < snapshot.json | ./scripts/marbles.sh --dry-run
#   arborist report --json < snapshot.json | ./scripts/marbles.sh
#
# `mb` is the Marbles command every host in the fleet already has on its PATH:
# a `wasi:cli` component run by `soil-command`, reading and writing the records
# book at `~/.local/share/fungos/records`. This script therefore talks to the
# tracker the way a person does — no HTTP client, no service token, no second
# route into the same data, and the same audit trail.
#
# It reconciles rather than appends:
#
#   * one task per repository that fails a CRITICAL check, titled
#     `arborist: OWNER/NAME`, so a second run finds the first;
#   * a task is left alone when it already exists — re-running is safe, and a
#     nightly job does not produce a new backlog every night;
#   * a task whose repository no longer fails is closed, because a tracker full
#     of fixed work is a tracker nobody reads.
#
# Advisory findings do not become tasks. Drift is not a decision; a missing
# licence or a red default branch is.
set -eu

project=${MARBLES_PROJECT:-arborist}
dry_run=0
limit=25
prefix="arborist: "
# The label is the identity of a task this script wrote. The title is for a
# person reading a list, and a title is not something a script may trust: the
# first version of this file matched by title prefix and closed the project's
# own epic, because "arborist: source-code health for the fleet" is exactly what
# a human writes too.
label=arborist

usage() {
  echo "usage: arborist report --json | marbles.sh [--project P] [--dry-run] [--limit N]" >&2
  echo "" >&2
  echo "  --limit N   create at most N tasks in one run (default 25, 0 means all)." >&2
  echo "              A first audit of a large fleet fails hundreds of checks and" >&2
  echo "              filing them all at once buries the backlog it was meant to" >&2
  echo "              create." >&2
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --project) project=$2; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    --limit) limit=$2; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "marbles.sh: unknown option $1" >&2; usage; exit 2 ;;
  esac
done

# `mb` is the command, not a library. If it is not here, say so and stop: a
# sync that silently files nothing is worse than one that refuses.
if ! command -v mb >/dev/null 2>&1; then
  echo "marbles.sh: no \`mb\` on PATH." >&2
  echo "  It is the Marbles command over soil:records, installed as a wasm command:" >&2
  echo "    soil-command shims --dir ~/.local/share/fungos/bin   # then put that on PATH" >&2
  echo "  A GitHub-hosted runner has no records book and cannot file tasks." >&2
  exit 2
fi

report=$(cat)
if [ -z "$report" ]; then
  echo "marbles.sh: no report on stdin" >&2
  exit 2
fi

critical=$(printf '%s' "$report" | jq -r '
  [ .repos[]
    | select(any(.findings[]; .status == "fail" and .severity == "critical"))
    | .org + "/" + .name ]
  | unique | .[]')

if [ -z "$critical" ]; then
  echo "marbles.sh: no repository fails a critical check"
fi

existing=$(mb --project "$project" list --json 2>/dev/null || echo '[]')
tracked=$(printf '%s' "$existing" | jq -r --arg prefix "$prefix" --arg label "$label" '
  .[]
  | select(.status == "open")
  | select(.labels | index($label))
  | select(.title | startswith($prefix))
  | .id + "\t" + (.title | ltrimstr($prefix))')

created=0
printf '%s\n' "$critical" | while IFS= read -r slug; do
  [ -n "$slug" ] || continue
  if printf '%s' "$tracked" | cut -f2 | grep -Fxq "$slug"; then
    continue
  fi
  if [ "$limit" -gt 0 ] && [ "$created" -ge "$limit" ]; then
    echo "marbles.sh: limit of $limit reached; the rest wait for the next run" >&2
    break
  fi
  body=$(printf '%s' "$report" | jq -r --arg slug "$slug" '
    .repos[] | select(.org + "/" + .name == $slug)
    | [ .findings[]
        | select(.status == "fail")
        | "- \(.check) (\(.severity)): \(.detail)" ]
    | join("\n")')
  if [ "$dry_run" -eq 1 ]; then
    echo "would create $prefix$slug"
  else
    mb --project "$project" create "$prefix$slug" --labels "$label" --description "$body" >/dev/null
    echo "created $prefix$slug"
  fi
  created=$((created + 1))
done

# Close the tasks whose repository stopped failing. `--ack` is the honest
# evidence: nothing was delivered, the finding is gone.
printf '%s' "$tracked" | while IFS=$'\t' read -r id slug; do
  [ -n "$id" ] || continue
  if printf '%s\n' "$critical" | grep -Fxq "$slug"; then
    continue
  fi
  if [ "$dry_run" -eq 1 ]; then
    echo "would close $prefix$slug ($id)"
  else
    mb close "$id" --ack "arborist: $slug no longer fails a critical check" >/dev/null
    echo "closed $prefix$slug ($id)"
  fi
done
