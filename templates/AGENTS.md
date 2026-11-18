<!-- BEGIN MARBLES integration -->

## Marbles issue tracker

This project tracks work with `marbles` (`mb`). Issues, dependencies and claims
live in the central records book — never in a markdown task list, and never in a
per-checkout database that has to be synced.

```bash
mb ready --json                         # what is eligible now
mb --project <slug> create "title"      # file work (projects are keyed by slug)
mb claim <id> --as <who>                # take it; claims race safely
mb touch <id>                           # heartbeat a lease mid-work
mb review <id> --pr URL                 # a PR exists: under review, NOT done
mb close <id> --commit SHA              # done means merged
mb close <id> --ack "no delivery expected: <why>"   # research or coordination
```

Closing needs evidence — a pull request, a commit, or an explicit
acknowledgement that nothing will ship. An agent that has finished writing code
has produced a review, not a delivery.

Use `mb` for all task tracking. Do not create markdown TODO lists, and do not
keep a task list in a file the fleet cannot see.

<!-- END MARBLES integration -->
