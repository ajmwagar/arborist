#!/bin/sh
# scan.sh: one whole audit pass — collect, score, keep the report, publish the
# headline. This is the command a scheduler runs.
#
#   ./collector/scan.sh FuturePresentLabs mycelium-linux
#   ./collector/scan.sh --repo OWNER/NAME
#
# It is written for `mycelium pulse`, which runs a bounded shell command on a
# cron and reports the exit code as the run's result. That shapes two things:
#
#   * **the exit code is about the scan, not the fleet.** A red fleet is a
#     successful scan. Exit 0 means "the audit ran and here is what it said";
#     non-zero means "the audit did not happen", which is the only thing a
#     scheduler's run history should be alarming about. The verdict travels in
#     the published event and in the report file.
#
#   * **it must fit in the scheduler's budget.** Pulse bounds a run at 60
#     seconds. `FLEET_DEADLINE` caps the collection at 50 by default so the
#     scan ends with a report instead of being killed with none.
#
# Environment:
#   ARBORIST_BIN     a native arborist binary (default: `arborist` on PATH)
#   ARBORIST_WASM    the component, run through `wasmtime` (used when set)
#   ARBORIST_REPORT  where the JSON report is written
#                    (default: ~/.local/state/arborist/report.json)
#   UNIBUS_CLIENT    a ClientConfig JSON; publishing is skipped when unset
#   UNIBUS_KIND      the message kind (default: arborist.scan)
#   FLEET_DEADLINE   seconds allowed for collection (default: 50)
set -eu

here=$(dirname "$0")
report=${ARBORIST_REPORT:-${XDG_STATE_HOME:-$HOME/.local/state}/arborist/report.json}
kind=${UNIBUS_KIND:-arborist.scan}
deadline=${FLEET_DEADLINE:-50}
work=${TMPDIR:-/tmp}/arborist-scan.$$
mkdir -p "$(dirname "$report")" || {
  echo "scan.sh: cannot create $(dirname "$report")" >&2
  exit 2
}
trap 'rm -f "$work"*' EXIT INT TERM

case "${1:-}" in
  -h|--help)
    sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
esac

if [ "$#" -eq 0 ]; then
  echo "usage: scan.sh ORG..." >&2
  echo "       scan.sh --repo OWNER/NAME" >&2
  exit 2
fi

# One way to run the scorer, whichever build is on this machine.
arborist() {
  if [ -n "${ARBORIST_WASM:-}" ]; then
    wasmtime run "$ARBORIST_WASM" "$@"
  else
    "${ARBORIST_BIN:-arborist}" "$@"
  fi
}

# 1. The forge, as one document. Bounded, because a scheduler will kill us
#    otherwise and a killed collection leaves nothing to publish.
if command -v timeout >/dev/null 2>&1; then
  collect() { timeout "$deadline" "$here/collect.sh" "$@"; }
elif command -v gtimeout >/dev/null 2>&1; then
  collect() { gtimeout "$deadline" "$here/collect.sh" "$@"; }
else
  # No timeout(1) on this box. Say so rather than pretending the cap exists.
  echo "scan.sh: no timeout(1); collection is not bounded here" >&2
  collect() { "$here/collect.sh" "$@"; }
fi

if ! collect "$@" > "$work.pages"; then
  echo "scan.sh: collection failed or exceeded ${deadline}s" >&2
  exit 2
fi

# 2. Pages to a snapshot, snapshot to a report. Both are the published
#    component: the scan scores with the same bytes CI gates on.
if ! arborist ingest < "$work.pages" > "$work.snapshot"; then
  echo "scan.sh: could not read the collected pages" >&2
  exit 2
fi
if ! arborist report --json --fail-on none < "$work.snapshot" > "$work.report"; then
  echo "scan.sh: could not score the snapshot" >&2
  exit 2
fi
mv -f "$work.report" "$report"

# 3. The paragraph a person reads and a bus can carry.
arborist digest < "$work.snapshot" > "$work.digest" || {
  echo "scan.sh: could not digest the report" >&2
  exit 2
}
headline=$(sed -n 's/^  "headline": "\(.*\)",$/\1/p' "$work.digest")
echo "arborist: $headline"
echo "arborist: report at $report"

# 4. Publish, when there is somewhere to publish to. Silence would be a lie
#    here: a scan nobody hears about is the thing this is supposed to fix.
if [ -n "${UNIBUS_CLIENT:-}" ]; then
  if ! unibus-publish --config "$UNIBUS_CLIENT" --kind "$kind" < "$work.digest" >/dev/null; then
    echo "scan.sh: the scan finished but could not be published" >&2
    exit 1
  fi
else
  echo "scan.sh: UNIBUS_CLIENT is unset; nothing was published" >&2
fi

exit 0
