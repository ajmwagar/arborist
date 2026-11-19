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
#     seconds. `FLEET_DEADLINE` caps the collection at 55 by default so the scan
#     ends with a report instead of being killed with none. It was 50 until the
#     branch audit: asking for the branches themselves costs about seventeen
#     seconds, and a measured two-owner run now takes 49.
#
# Environment:
#   ARBORIST_BIN     a native arborist binary (default: `arborist` on PATH)
#   ARBORIST_WASM    the component, run through `wasmtime` (used when set)
#   ARBORIST_REPORT  where the JSON report is written
#                    (default: ~/.local/state/arborist/report.json)
#   UNIBUS_CLIENT    a ClientConfig JSON; publishing is skipped when unset
#   UNIBUS_KIND      the message kind for the digest (default: arborist.scan)
#   UNIBUS_EVENTS_KIND  the kind for per-repository events (default:
#                    arborist.finding)
#   UNIBUS_EVENTS    set to 0 to publish only the digest, not the events
#   UNIBUS_JOBS      how many events are in flight at once (default: 8)
#   FLEET_DEADLINE   seconds allowed for collection (default: 55)
set -eu

here=$(dirname "$0")
report=${ARBORIST_REPORT:-${XDG_STATE_HOME:-$HOME/.local/state}/arborist/report.json}
kind=${UNIBUS_KIND:-arborist.scan}
events_kind=${UNIBUS_EVENTS_KIND:-arborist.finding}
events_on=${UNIBUS_EVENTS:-1}
jobs=${UNIBUS_JOBS:-8}
deadline=${FLEET_DEADLINE:-55}
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
  # No timeout(1) on this box — macOS has none by default. A scheduler that
  # kills the run at 60 seconds turns an unbounded collection into a scan with
  # no report, so the bound is done here instead of being wished for.
  collect() {
    "$here/collect.sh" "$@" &
    collector=$!
    ( sleep "$deadline"; kill -TERM "$collector" 2>/dev/null ) &
    watchdog=$!
    wait "$collector"
    status=$?
    kill -TERM "$watchdog" 2>/dev/null
    wait "$watchdog" 2>/dev/null
    return $status
  }
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
#
#    Two shapes, because the bus serves two kinds of reader. The digest is one
#    message a person or a dashboard reads whole. The events are one message per
#    failing repository, so a subscriber that owns one repository receives that
#    one and does not have to filter the fleet out of a summary that names ten.
if [ -z "${UNIBUS_CLIENT:-}" ]; then
  echo "scan.sh: UNIBUS_CLIENT is unset; nothing was published" >&2
  exit 0
fi

if ! unibus-publish --config "$UNIBUS_CLIENT" --kind "$kind" < "$work.digest" >/dev/null; then
  echo "scan.sh: the scan finished but could not be published" >&2
  exit 1
fi
published=1

if [ "$events_on" != "0" ]; then
  if ! arborist events < "$work.snapshot" > "$work.events"; then
    echo "scan.sh: the scan finished but the events could not be rendered" >&2
    exit 1
  fi

  # Bounded concurrency for the same reason a bounded collection: 150 messages
  # is 150 process starts and connections, and the scheduler's budget does not
  # grow because the fleet did.
  running=0
  : > "$work.failures"
  # The loop reads on fd 3, not stdin: a publisher that reads stdin would
  # otherwise eat the events it has not been handed yet, and the loop would end
  # early having published one message and reported success.
  while IFS= read -r line <&3; do
    (
      if printf '%s' "$line" \
        | unibus-publish --config "$UNIBUS_CLIENT" --kind "$events_kind" >/dev/null 2>&1
      then exit 0
      else echo x >> "$work.failures"
      fi
    ) </dev/null &
    running=$((running + 1))
    if [ "$running" -ge "$jobs" ]; then
      wait || true
      running=0
    fi
  done 3< "$work.events"
  wait || true

  sent=$(wc -l < "$work.events" | tr -d ' ')
  failures=0
  if [ -f "$work.failures" ]; then
    failures=$(wc -l < "$work.failures" | tr -d ' ')
  fi
  if [ "$failures" -gt 0 ]; then
    echo "scan.sh: $failures of $sent events could not be published" >&2
    exit 1
  fi
  echo "scan.sh: published $sent event(s) as $events_kind"
fi

exit 0
