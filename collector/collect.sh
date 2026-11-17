#!/bin/sh
# collect.sh: the inputs to one arborist report, for a machine with `gh`.
#
#   ./collector/collect.sh FuturePresentLabs mycelium-linux > snapshot.json
#   ./collector/collect.sh --repo FuturePresentLabs/bob > snapshot.json
#   arborist report < snapshot.json
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
fi

for org in "$@"; do
  echo "collecting $org" >&2
  query="$fields$(cat "$here/fleet.graphql")"
  fetch --paginate -f org="$org" -f "query=$query"
done
