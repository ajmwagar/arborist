#!/bin/sh
# collect.sh: the inputs to one arborist report, for a machine with `gh`.
#
#   ./collector/collect.sh FuturePresentLabs mycelium-linux > snapshot.json
#   ./collector/collect.sh --repo FuturePresentLabs/bob > snapshot.json
#   arborist report < snapshot.json
#
# Owners are collected in parallel because they are independent queries. One
# owner's pages are not: the cursor is the only way to ask for the next one. So
# two owners cost the slower of the two rather than the sum, and one owner costs
# what it always did.
#
# This is the reference collector, not the only one. It exists to prove the
# snapshot contract is writable by anything: a shell here, a Spore over
# `soil:http` later, a scheduled job on a box that already holds a token. The
# mapping from forge payload to snapshot is in `arborist ingest`, not in this
# script, which is why the script is short and the mapping has tests.
#
# The token is the caller's: `gh` already holds one, and arborist never sees it.
set -eu

here=$(dirname "$0")
fields=$(cat "$here/fields.graphql")

if [ "$#" -eq 0 ]; then
  echo "usage: collect.sh ORG..." >&2
  echo "       collect.sh --repo OWNER/NAME" >&2
  exit 2
fi

# One owner's pages, with retries.
#
# The proxy in front of the GraphQL API answers a large page with 502 often
# enough that a nightly job which does not retry fails at random. Four attempts
# with a growing pause turn that into a slow success, and a real error still
# fails loudly at the end.
fetch() {
  attempt=1
  while [ "$attempt" -le 4 ]; do
    if out=$(gh api graphql "$@" 2>/dev/null) && [ -n "$out" ]; then
      case "$out" in
        "<html>"* | *"502 Bad Gateway"* | '{"errors"'*) : ;;
        *) printf '%s\n' "$out"; return 0 ;;
      esac
    fi
    attempt=$((attempt + 1))
    [ "$attempt" -le 4 ] && sleep "$((attempt * 2))"
  done
  echo "collect.sh: could not collect after 3 retries" >&2
  return 1
}

if [ "$1" = "--repo" ]; then
  [ "$#" -eq 2 ] || { echo "collect.sh: --repo takes OWNER/NAME" >&2; exit 2; }
  owner=${2%%/*}
  name=${2#*/}
  [ "$owner" != "$2" ] || { echo "collect.sh: --repo expects OWNER/NAME" >&2; exit 2; }
  query="$fields$(cat "$here/repo.graphql")"
  fetch -f owner="$owner" -f name="$name" -f "query=$query"
  # Without this the loop below collects "--repo" and "OWNER/NAME" as owners.
  # GitHub answers those with data-less documents, `fetch` accepts them because
  # they are not errors in the shape it retries, and the ingest skips them — so
  # one repository quietly cost three requests and two of them were nonsense.
  exit 0
fi

# Owners in flight at once. Four is enough to hide one owner's latency behind
# another's without turning a rate limit into a mystery, and the fleet has two
# owners today, so this is mostly headroom.
jobs=${ARBORIST_COLLECT_JOBS:-4}
work=${TMPDIR:-/tmp}/arborist-collect.$$
trap 'rm -f "$work".*' EXIT INT TERM

collected=0
running=0
for org in "$@"; do
  collected=$((collected + 1))
  (
    echo "collecting $org" >&2
    query="$fields$(cat "$here/fleet.graphql")"
    fetch --paginate -f org="$org" -f "query=$query" > "$work.$collected"
  ) &
  running=$((running + 1))
  if [ "$running" -ge "$jobs" ]; then
    # One owner failing fails the collection: a report that silently covers one
    # of two owners would close the other owner's tasks as "no longer scored".
    wait || exit 1
    running=0
  fi
done
wait || exit 1

# In argument order, not completion order, so the same fleet always produces the
# same document and two snapshots are worth diffing.
i=1
while [ "$i" -le "$collected" ]; do
  cat "$work.$i"
  i=$((i + 1))
done
