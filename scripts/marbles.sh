#!/bin/sh
# marbles.sh: file one Marbles task per failing check, in the project of the
# repository that failed it.
#
#   arborist report --json | ./scripts/marbles.sh --dry-run
#   arborist report --json | ./scripts/marbles.sh
#
# `mb` is the Marbles command every host already has: a `wasi:cli` component run
# by `soil-command` over the records book at `~/.local/share/fungos/records`. So
# this writes the tracker the way a person does — same audit trail, no service
# token, no second route into the same data.
#
# ## Per repository, because that is where it gets fixed
#
# One central "133 repositories fail a critical check" task says that a
# repository somewhere is missing a licence. Nobody owns it, and nothing runs
# `mb ready` on it. A task in the repository's own project is in front of the
# person who can fix it, in the place they already look:
#
#     arborist: license        in FuturePresentLabs/bob
#     arborist: ci             in FuturePresentLabs/bob
#
# The title is the check and nothing else, so the same failure found twice is
# the same task, and a detail that changes (183 branches becoming 190) rewrites
# nothing and opens nothing.
#
# ## Which project a repository lands in
#
#   * the project for `owner/name`, if the registry has one — unambiguous;
#   * otherwise the bare `name`, if the registry has it *and* no other
#     repository in this report shares that name — so a repository with a local
#     checkout keeps the backlog it already has;
#   * otherwise `owner/name`, which `mb` creates on first use.
#
# ## It reconciles rather than appends
#
#   * a task that is already open is left alone, so a nightly run does not
#     produce a new backlog every night;
#   * a task whose check now passes is closed, because a tracker full of fixed
#     work is a tracker nobody reads;
#   * a repository that has left the report — archived, deleted, unscored —
#     closes too: a task nobody can act on is not work;
#   * a task titled after a repository rather than a check is the superseded
#     shape this script used to file, and closes saying where the work went.
#
# ## Two `mb` calls, then only the deltas
#
# A call to `mb` costs about twelve seconds: it starts a wasm component against a
# 290 MB records book. So the current state is read twice (`projects`, `list`),
# reconciled in memory, and only tasks being created or closed pay for a call.
# A night where nothing changed is two calls.
#
# Identity is the LABEL, not the title. The first version of this file matched
# by title prefix and closed the project's own epic, because "arborist: source-
# code health for the fleet" is exactly what a human writes too.
set -eu

dry_run=0
limit=0
prefix="arborist: "
label=arborist
# Which findings become tasks. `critical` is the default because a fleet audit
# fails hundreds of advisory checks, and filing them all buries the backlog it
# was meant to create.
fail_on=critical
# `mb` costs most of its twelve seconds starting up, so creating a backlog one
# call at a time takes an hour. Eight at once took 243 tasks in about three
# minutes and lost none of them: the records book serialises the writes itself.
jobs=8

usage() {
  cat <<'USAGE'
usage: arborist report --json | marbles.sh [options]
       marbles.sh [options] REPORT.json...

  A report on stdin, or one report file per argument. The schedules write one
  report per owner, so pass them all: they are merged before anything is
  decided, because a repository named the same in two owners is two
  repositories and the project rule has to see every owner at once.

  --dry-run        print what would be filed and closed; change nothing
  --limit N        file at most N tasks in one run (default 0: no limit)
  --fail-on LEVEL  critical (default) or advisory
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run) dry_run=1; shift ;;
    --limit) limit=$2; shift 2 ;;
    --fail-on) fail_on=$2; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    --) shift; break ;;
    -*) echo "marbles.sh: unknown option $1" >&2; usage >&2; exit 2 ;;
    *) break ;;
  esac
done

case "$fail_on" in
  critical|advisory) ;;
  *) echo "marbles.sh: --fail-on takes critical or advisory" >&2; exit 2 ;;
esac

# `mb` is the command, not a library. If it is not here, say so and stop: a sync
# that silently files nothing is worse than one that refuses.
if ! command -v mb >/dev/null 2>&1; then
  echo "marbles.sh: no \`mb\` on PATH." >&2
  echo "  It is the Marbles command over soil:records, installed as a wasm command:" >&2
  echo "    soil-command shims --dir ~/.local/share/fungos/bin   # then put that on PATH" >&2
  echo "  A GitHub-hosted runner has no records book and cannot file tasks." >&2
  exit 2
fi

work=${TMPDIR:-/tmp}/arborist-marbles.$$
trap 'rm -f "$work".*' EXIT INT TERM

# One report in, one document out, however many owners the fleet has. `jq -s`
# over the files is a merge and not a concatenation: `repos` stays one array, so
# the name-collision rule below counts every owner's repositories together.
if [ "$#" -gt 0 ]; then
  for file in "$@"; do
    if [ ! -r "$file" ]; then
      echo "marbles.sh: cannot read $file" >&2
      exit 2
    fi
  done
  if ! jq -s '
    {
      schema: (.[0].schema // null),
      collected_at: (.[0].collected_at // null),
      policy: (.[0].policy // null),
      repos: [.[].repos[]]
    }' "$@" > "$work.report"; then
    echo "marbles.sh: could not read the reports given" >&2
    exit 2
  fi
else
  cat > "$work.report"
fi
if [ ! -s "$work.report" ]; then
  echo "marbles.sh: no report on stdin and none named on the command line" >&2
  exit 2
fi

# Projects that already exist, lowercased slug -> slug as `mb` spells it.
mb projects --json 2>/dev/null > "$work.projects" || echo '[]' > "$work.projects"
jq -r '.[] | select((.slug // "") != "") | "\(.slug | ascii_downcase)\t\(.slug)"' \
  < "$work.projects" > "$work.slugs"

# Every arborist task still open, anywhere: project, id, check.
mb list --json 2>/dev/null > "$work.all" || echo '[]' > "$work.all"
jq -r --arg label "$label" --arg prefix "$prefix" '
  .[] | select(.status == "open")
  | select(.labels | index($label))
  | select(.title | startswith($prefix))
  | [.project, .id, (.title | ltrimstr($prefix))] | @tsv' \
  < "$work.all" > "$work.open"

# Resolve each scored repository to a project, then emit one record per
# repository and per violation: a single jq program, so the rule lives once.
jq -r --arg fail_on "$fail_on" --rawfile slugs "$work.slugs" '
  def keep(f):
    f.status == "fail"
    and (($fail_on == "critical" and f.severity == "critical")
         or ($fail_on == "advisory"));
  def clean: gsub("[\\t\\n\\r]+"; " ");

  ($slugs | split("\n") | map(select(length > 0) | split("\t"))
   | map({ key: .[0], value: .[1] }) | from_entries) as $registry
  | ([.repos[].name] | group_by(.) | map({ key: .[0], value: length }) | from_entries) as $names
  | .repos[]
  | . as $repo
  | ($repo.org + "/" + $repo.name) as $full
  | (if $registry[$full | ascii_downcase] then $registry[$full | ascii_downcase]
     elif ($registry[$repo.name | ascii_downcase] and $names[$repo.name] == 1)
       then $registry[$repo.name | ascii_downcase]
     else $full end) as $project
  | "P\t\($project)\t\($full)",
    ( $repo.findings[] | select(keep(.))
      | "V\t\($project)\t\($full)\t\(.check)\t\(.severity)\t\((.detail // "") | clean)" )
' "$work.report" > "$work.records"

: > "$work.map"          # project -> repository it belongs to, this report
: > "$work.violations"   # project, repository, check, severity, detail
: > "$work.checks"       # every check name the audit can emit
tab=$(printf '\t')
while IFS="$tab" read -r kind a b c d e; do
  case "$kind" in
    P) printf '%s\t%s\n' "$a" "$b" >> "$work.map" ;;
    V) printf '%s\t%s\t%s\t%s\t%s\n' "$a" "$b" "$c" "$d" "$e" >> "$work.violations"
       printf '%s\n' "$c" >> "$work.checks" ;;
  esac
done < "$work.records"

created=0
closed=0
running=0

# File what is failing and not yet filed.
while IFS="$tab" read -r project repo check severity detail; do
  [ -n "$project" ] || continue
  if awk -F'\t' -v p="$project" -v c="$check" '
       $1 == p && $2 == c { found = 1 } END { exit !found }' "$work.open"
  then
    continue
  fi
  if [ "$limit" -gt 0 ] && [ "$created" -ge "$limit" ]; then
    echo "marbles.sh: limit of $limit reached; the rest wait for the next run" >&2
    break
  fi

  body="$repo

$severity: $detail

Filed by the arborist fleet audit. One task per failing check, in this
repository's own project, so the fix sits with the repository that needs it.
It closes when \`arborist: $check\` passes for $repo."

  if [ "$dry_run" -eq 1 ]; then
    echo "would file  $prefix$check  in $project  ($repo)"
  else
    # `wait` reaps every child, so this is a pool of at most $jobs writers
    # rather than a batch: the next create starts as soon as one finishes.
    (
      if mb --project "$project" create "$prefix$check" --labels "$label" \
           --description "$body" >/dev/null 2>&1
      then echo "filed       $prefix$check  in $project  ($repo)"
      else echo "FAILED      $prefix$check  in $project  ($repo)" >&2
           echo "$project $check" >> "$work.failed"
      fi
    ) &
    running=$((running + 1))
    if [ "$running" -ge "$jobs" ]; then
      wait || true
      running=0
    fi
  fi
  created=$((created + 1))
done < "$work.violations"
wait || true

# Close what has stopped failing, or was filed in the shape this replaced.
while IFS="$tab" read -r project id check; do
  [ -n "$id" ] || continue

  # A title that is not a check name is a repository: the old shape, one task
  # per repository in the central project. Its work now belongs to that
  # repository's own project.
  if ! grep -Fxq "$check" "$work.checks"; then
    if [ "$dry_run" -eq 1 ]; then
      echo "would close $id  in $project  (superseded: filed per repository now)"
    else
      mb close "$id" --ack "superseded: arborist files one task per failing check in each repository's own project now" >/dev/null
      echo "closed      $id  in $project  (superseded: filed per repository now)"
    fi
    closed=$((closed + 1))
    continue
  fi

  repo=$(awk -F'\t' -v p="$project" '$1 == p { print $2; exit }' "$work.map")
  if [ -n "$repo" ] && awk -F'\t' -v r="$repo" -v c="$check" '
       $2 == r && $3 == c { found = 1 } END { exit !found }' "$work.violations"
  then
    continue
  fi

  if [ "$dry_run" -eq 1 ]; then
    echo "would close $id  $prefix$check  in $project"
  else
    mb close "$id" --ack "arborist: $check passes for ${repo:-$project}, or it is no longer scored" >/dev/null
    echo "closed      $id  $prefix$check  in $project"
  fi
  closed=$((closed + 1))
done < "$work.open"

echo "marbles.sh: filed $created, closed $closed" >&2
failed=0
if [ -f "$work.failed" ]; then
  failed=$(wc -l < "$work.failed" | tr -d ' ')
fi
if [ "$failed" -gt 0 ]; then
  echo "marbles.sh: $failed create(s) failed; run again to fill the gaps" >&2
  exit 1
fi
