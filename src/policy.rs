//! The rules, and the one function that applies them.
//!
//! [`evaluate`] is a pure function from a [`Snapshot`] to a [`Report`]. It does
//! not read the clock, open a socket, or look at a path, so the answer a shell
//! command prints and the answer a view draws are the same answer, byte for
//! byte, from the same bytes.

use serde::{Deserialize, Serialize};

use crate::check::{Check, Severity, Status};
use crate::date::days_between;
use crate::report::{RepoReport, Report, SkipReason, Skipped};
use crate::snapshot::{RepoState, Snapshot};

/// The thresholds and the scope. Tuning the fleet is editing this, not a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    /// A repository whose last push is older than this is drifting. 90 days is
    /// one quarter: long enough to be a quiet project rather than a bad week.
    pub stale_days: i64,
    /// Branches per repository before the count is worth a look.
    pub max_branches: u32,
    /// Open pull requests per repository before the count is worth a look.
    pub max_open_pulls: u32,
    /// How long a branch may carry work nobody has opened a pull request for.
    /// Twelve hours is one working day: a branch pushed this morning is a
    /// branch in progress, and one pushed yesterday is work nobody can review.
    pub stale_branch_hours: i64,
    /// Whether an archived repository is skipped. Off by default because an
    /// archived repository is a decision somebody already made.
    pub check_archived: bool,
    /// Whether a fork is skipped. A fork is somebody else's repository with a
    /// local ref, and auditing it says more about upstream than about us.
    pub check_forks: bool,
    /// A README smaller than this is a placeholder. A title and a sentence fit
    /// in 256 bytes; anything shorter is a file somebody created to stop the
    /// forge complaining.
    pub min_readme_bytes: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            stale_days: 90,
            max_branches: 50,
            max_open_pulls: 10,
            stale_branch_hours: 12,
            check_archived: false,
            check_forks: false,
            min_readme_bytes: 256,
        }
    }
}

/// One check's verdict on one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub check: Check,
    pub severity: Severity,
    #[serde(flatten)]
    pub status: Status,
}

impl Finding {
    fn pass(check: Check) -> Self {
        Self {
            check,
            severity: check.severity(),
            status: Status::Pass,
        }
    }

    fn fail(check: Check, detail: impl Into<String>) -> Self {
        Self {
            check,
            severity: check.severity(),
            status: Status::Fail {
                detail: detail.into(),
            },
        }
    }

    fn unknown(check: Check, detail: impl Into<String>) -> Self {
        Self {
            check,
            severity: check.severity(),
            status: Status::Unknown {
                detail: detail.into(),
            },
        }
    }
}

/// Apply every check to every repository.
#[must_use]
pub fn evaluate(snapshot: &Snapshot) -> Report {
    let mut repos = Vec::new();
    let mut skipped = Vec::new();

    for repo in &snapshot.repos {
        let skip = if repo.archived && !snapshot.policy.check_archived {
            Some(SkipReason::Archived)
        } else if repo.fork && !snapshot.policy.check_forks {
            Some(SkipReason::Fork)
        } else {
            None
        };
        if let Some(reason) = skip {
            skipped.push(Skipped {
                org: repo.org.clone(),
                name: repo.name.clone(),
                reason,
            });
            continue;
        }

        let findings = Check::ALL
            .into_iter()
            .map(|check| match repo.exempt.get(check.id()) {
                Some(reason) => Finding {
                    check,
                    severity: check.severity(),
                    status: Status::Exempt {
                        reason: reason.clone(),
                    },
                },
                None => evaluate_one(
                    check,
                    repo,
                    &snapshot.policy,
                    snapshot.collected_at.as_deref(),
                ),
            })
            .collect();

        repos.push(RepoReport {
            org: repo.org.clone(),
            name: repo.name.clone(),
            findings,
        });
    }

    // A stable order is not cosmetic: a diff between two reports is how a fleet
    // reads, and the forge's own ordering is by name within an owner only.
    repos.sort_by_key(|left| left.slug());
    skipped.sort_by_key(|left| left.slug());

    Report::new(
        snapshot.schema,
        snapshot.collected_at.clone(),
        snapshot.policy.clone(),
        snapshot.orgs(),
        repos,
        skipped,
    )
}

fn evaluate_one(
    check: Check,
    repo: &RepoState,
    policy: &Policy,
    collected_at: Option<&str>,
) -> Finding {
    match check {
        Check::License => match &repo.license {
            Some(spdx) if !spdx.trim().is_empty() => Finding::pass(check),
            _ => Finding::fail(check, "no licence file the forge can name"),
        },
        Check::Ci => match repo.workflows.len() {
            0 => Finding::fail(check, "no .github/workflows entry"),
            _ => Finding::pass(check),
        },
        Check::CiGreen => match &repo.latest_run {
            None => Finding::unknown(check, "no workflow run was collected"),
            Some(run) => match run.conclusion.as_str() {
                "success" => Finding::pass(check),
                "" => Finding::unknown(check, "the last run has not concluded"),
                other => Finding::fail(check, format!("last run concluded {other}")),
            },
        },
        Check::Release => match repo.releases {
            0 => Finding::fail(check, "no releases"),
            _ => Finding::pass(check),
        },
        Check::Readme => match &repo.readme {
            None => Finding::unknown(check, "the README was not read"),
            Some(readme) if !readme.present => {
                Finding::fail(check, "no README on the default branch")
            }
            Some(readme) if readme.bytes < policy.min_readme_bytes => Finding::fail(
                check,
                format!(
                    "the README is {} bytes, under the {} that says what it is",
                    readme.bytes, policy.min_readme_bytes
                ),
            ),
            Some(_) => Finding::pass(check),
        },
        Check::Badges => match &repo.readme {
            None => Finding::unknown(check, "the README was not read"),
            // A repository with no README already fails `readme`; failing here
            // as well would report one absence twice.
            Some(readme) if !readme.present => {
                Finding::unknown(check, "there is no README to carry a badge")
            }
            Some(readme) => evaluate_badges(check, repo, &readme.badges),
        },
        Check::Description => match repo.description.as_deref().map(str::trim) {
            Some(description) if !description.is_empty() => Finding::pass(check),
            _ => Finding::fail(check, "the forge description is empty"),
        },
        Check::DocsLinks => match &repo.links {
            None => Finding::unknown(check, "no link check was collected"),
            Some(links) => {
                let broken: Vec<&crate::snapshot::Link> =
                    links.iter().filter(|link| !link_ok(link)).collect();
                if broken.is_empty() {
                    Finding::pass(check)
                } else {
                    let sample: Vec<String> = broken
                        .iter()
                        .take(3)
                        .map(|link| match (link.status, &link.error) {
                            (Some(status), _) => format!("{} -> {status}", link.url),
                            (None, Some(error)) => format!("{} -> {error}", link.url),
                            (None, None) => link.url.clone(),
                        })
                        .collect();
                    Finding::fail(
                        check,
                        format!(
                            "{} of {} do not resolve: {}",
                            broken.len(),
                            links.len(),
                            sample.join("; ")
                        ),
                    )
                }
            }
        },
        Check::Tracker => match &repo.tracker {
            None => Finding::unknown(check, "no agent instructions were read"),
            // No AGENTS.md and no CLAUDE.md is a repository whose instructions
            // are certainly not stale. Whether it *should* carry instructions
            // is a different question, and one this check does not ask.
            Some(tracker) if !tracker.unread.is_empty() && tracker.stale.is_empty() => {
                Finding::unknown(
                    check,
                    format!(
                        "the forge would not return {} as text",
                        tracker.unread.join(", ")
                    ),
                )
            }
            Some(tracker) if tracker.stale.is_empty() => Finding::pass(check),
            Some(tracker) => {
                let sample: Vec<String> = tracker
                    .stale
                    .iter()
                    .take(3)
                    .map(|stale| format!("`{}` in {}", stale.token, stale.path))
                    .collect();
                Finding::fail(
                    check,
                    format!(
                        "{} stale reference(s) to a tracker the fleet left: {}. \
                         The fleet uses {}",
                        tracker.stale.len(),
                        sample.join("; "),
                        crate::tracker::CURRENT
                    ),
                )
            }
        },
        Check::Metrics => match &repo.service {
            None => Finding::unknown(check, "no manifest was read"),
            // A library or a command has no endpoint to scrape. Failing it
            // would drown the services that do in the ones that could not.
            Some(service) if !service.serves() => Finding::pass(check),
            Some(service) if !service.metrics.is_empty() => Finding::pass(check),
            Some(service) => Finding::fail(
                check,
                format!(
                    "serves HTTP ({}) with no Prometheus client in the manifests",
                    service.http.join(", ")
                ),
            ),
        },
        Check::Alerts => match &repo.service {
            None => Finding::unknown(check, "no manifest was read"),
            // Nothing is exported, so there is nothing to alert on. A rule for
            // a series nobody publishes fires never or fires wrong.
            Some(service) if service.metrics.is_empty() => Finding::pass(check),
            Some(service) if !service.alerts.is_empty() => Finding::pass(check),
            Some(service) => Finding::fail(
                check,
                format!(
                    "exports {} with no alert rules; {}",
                    service.metrics.join(", "),
                    crate::service::ALERT_PATHS.join(", ")
                ),
            ),
        },
        Check::Active => match (collected_at, repo.pushed_at.as_deref()) {
            (None, _) => Finding::unknown(check, "the snapshot has no collected_at"),
            (_, None) => Finding::unknown(check, "no push date was collected"),
            (Some(collected_at), Some(pushed_at)) => match days_between(pushed_at, collected_at) {
                None => Finding::unknown(check, "the push date is unreadable"),
                Some(age) if age > policy.stale_days => Finding::fail(
                    check,
                    format!("last push was {age} days ago, limit {}", policy.stale_days),
                ),
                Some(_) => Finding::pass(check),
            },
        },
        Check::StaleBranches => match &repo.branch_list {
            None => Finding::unknown(check, "no branch list was collected"),
            Some(branches) => {
                // The default branch is where work lands, not work waiting.
                let looked_at = branches
                    .iter()
                    .filter(|branch| Some(branch.name.as_str()) != repo.default_branch.as_deref())
                    .collect::<Vec<_>>();
                let unmerged = looked_at.iter().filter(|branch| !branch.merged).count();
                let stale: Vec<&crate::Branch> = looked_at
                    .iter()
                    .copied()
                    .filter(|branch| branch.pulls == 0)
                    .filter(
                        |branch| match (collected_at, branch.committed_at.as_deref()) {
                            (Some(collected_at), Some(committed_at)) => {
                                crate::date::hours_between(committed_at, collected_at)
                                    .is_some_and(|hours| hours > policy.stale_branch_hours)
                            }
                            // An unreadable date is not evidence of staleness.
                            _ => false,
                        },
                    )
                    .collect();

                if stale.is_empty() {
                    Finding::pass(check)
                } else {
                    let names: Vec<String> = stale
                        .iter()
                        .take(3)
                        .map(|branch| branch.name.clone())
                        .collect();
                    Finding::fail(
                        check,
                        format!(
                            "{} branch(es) with no pull request for over {}h ({}); \
                             {} of {} unmerged",
                            stale.len(),
                            policy.stale_branch_hours,
                            names.join(", "),
                            unmerged,
                            looked_at.len()
                        ),
                    )
                }
            }
        },
        Check::Branches => {
            if repo.branches > policy.max_branches {
                Finding::fail(
                    check,
                    format!("{} branches, limit {}", repo.branches, policy.max_branches),
                )
            } else {
                Finding::pass(check)
            }
        }
        Check::OpenPulls => {
            if repo.open_pulls > policy.max_open_pulls {
                Finding::fail(
                    check,
                    format!(
                        "{} open pull requests, limit {}",
                        repo.open_pulls, policy.max_open_pulls
                    ),
                )
            } else {
                Finding::pass(check)
            }
        }
    }
}

/// A badge is only worth having if the thing it watches still exists.
///
/// The order is the order of how badly it lies: a dead service is a broken image
/// on the front page, a missing CI badge means nothing is watched, and a badge
/// pointing at a deleted workflow is a promise with no object.
fn evaluate_badges(check: Check, repo: &RepoState, badges: &[crate::Badge]) -> Finding {
    let mut dead: Vec<String> = Vec::new();
    let mut live_ci = 0usize;
    let mut missing_workflow: Vec<String> = Vec::new();

    for badge in badges {
        match crate::badge::classify(&badge.url) {
            crate::badge::BadgeKind::Dead { host } => dead.push(host),
            crate::badge::BadgeKind::Ci { .. } => live_ci += 1,
            crate::badge::BadgeKind::Actions { workflow, legacy } => {
                live_ci += 1;
                if let Some(workflow) = workflow {
                    if !legacy && !repo.workflows.contains(&workflow) {
                        // A legacy badge names the workflow's title, not its
                        // file, so there is nothing to compare it with.
                        missing_workflow.push(workflow);
                    }
                }
            }
            crate::badge::BadgeKind::Other => {}
        }
    }

    if !dead.is_empty() {
        dead.sort_unstable();
        dead.dedup();
        return Finding::fail(
            check,
            format!(
                "{} badge(s) watch a service that no longer exists: {}",
                dead.len(),
                dead.join(", ")
            ),
        );
    }
    if live_ci == 0 {
        return Finding::fail(
            check,
            format!(
                "no CI status badge; the README embeds {} other image(s)",
                badges.len()
            ),
        );
    }
    if !missing_workflow.is_empty() {
        return Finding::fail(
            check,
            format!(
                "a badge points at {}, which is not in .github/workflows",
                missing_workflow.join(", ")
            ),
        );
    }
    if repo.workflows.is_empty() {
        // The badges name a workflow and the file listing has none: the listing
        // and the README cannot both be right, and the README is the one that
        // was written by hand.
        return Finding::fail(
            check,
            "the README advertises CI and .github/workflows is empty",
        );
    }
    Finding::pass(check)
}

fn link_ok(link: &crate::snapshot::Link) -> bool {
    link.error.is_none()
        && link
            .status
            .is_some_and(|status| (200..400).contains(&status))
}

#[cfg(test)]
mod tests {
    use super::{Finding, Policy, evaluate};
    use crate::Badge;
    use crate::check::{Check, Status};
    use crate::report::SkipReason;
    use crate::snapshot::{Link, Readme, RepoState, Run, Snapshot};

    fn repo() -> RepoState {
        RepoState {
            org: "fpl".into(),
            name: "bob".into(),
            pushed_at: Some("2026-11-17T00:00:00Z".into()),
            license: Some("MIT".into()),
            description: Some("a build plane".into()),
            releases: 3,
            branches: 4,
            workflows: vec!["ci.yml".into()],
            ..RepoState::default()
        }
    }

    fn snapshot(repos: Vec<RepoState>) -> Snapshot {
        Snapshot {
            schema: crate::SCHEMA,
            collected_at: Some("2026-11-17T12:00:00Z".into()),
            policy: Policy::default(),
            repos,
        }
    }

    fn find(report: &crate::Report, check: Check) -> &Finding {
        report.repos[0]
            .findings
            .iter()
            .find(|finding| finding.check == check)
            .expect("every check is evaluated for every repository")
    }

    #[test]
    fn a_healthy_repository_passes_everything_it_has_evidence_for() {
        let report = evaluate(&snapshot(vec![repo()]));
        assert_eq!(find(&report, Check::License).status, Status::Pass);
        assert_eq!(find(&report, Check::Ci).status, Status::Pass);
        assert_eq!(find(&report, Check::Release).status, Status::Pass);
        assert_eq!(find(&report, Check::Active).status, Status::Pass);
        // No evidence was collected for these two, so they cannot pass.
        assert!(find(&report, Check::CiGreen).status.is_unknown());
        assert!(find(&report, Check::DocsLinks).status.is_unknown());
    }

    #[test]
    fn an_empty_repository_fails_the_critical_three() {
        let bare = RepoState {
            pushed_at: Some("2026-11-17T00:00:00Z".into()),
            ..RepoState {
                org: "fpl".into(),
                name: "bare".into(),
                ..RepoState::default()
            }
        };
        let report = evaluate(&snapshot(vec![bare]));
        assert!(find(&report, Check::License).status.is_fail());
        assert!(find(&report, Check::Ci).status.is_fail());
        assert!(find(&report, Check::Release).status.is_fail());
        assert!(find(&report, Check::Description).status.is_fail());
    }

    fn with_service(repo: &mut RepoState, http: &[&str], metrics: &[&str], alerts: &[&str]) {
        repo.service = Some(crate::Service {
            http: http.iter().map(|s| (*s).to_string()).collect(),
            metrics: metrics.iter().map(|s| (*s).to_string()).collect(),
            alerts: alerts.iter().map(|s| (*s).to_string()).collect(),
        });
    }

    fn branch(name: &str, committed_at: &str, pulls: u32, merged: bool) -> crate::Branch {
        crate::Branch {
            name: name.into(),
            committed_at: Some(committed_at.into()),
            pulls,
            merged,
        }
    }

    #[test]
    fn a_branch_with_no_pull_request_across_a_working_day_fails() {
        // `collected_at` in these tests is 2026-11-17T12:00:00Z.
        let mut repo = repo();
        repo.branch_list = Some(vec![
            branch("feature/half-done", "2026-11-16T00:00:00Z", 0, false),
            branch("feature/opened", "2026-11-10T00:00:00Z", 1, false),
        ]);
        let report = evaluate(&snapshot(vec![repo]));
        match &find(&report, Check::StaleBranches).status {
            Status::Fail { detail } => {
                assert!(detail.contains("feature/half-done"), "{detail}");
                assert!(!detail.contains("feature/opened"), "{detail}");
                // Both are unmerged: one because nobody opened a pull request,
                // one because it is still open. The count answers "how much
                // work is not landed", which is the question being asked.
                assert!(detail.contains("2 of 2 unmerged"), "{detail}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_branch_pushed_this_morning_is_work_in_progress_not_a_finding() {
        let mut repo = repo();
        repo.branch_list = Some(vec![branch(
            "feature/now",
            "2026-11-17T09:00:00Z",
            0,
            false,
        )]);
        let report = evaluate(&snapshot(vec![repo]));
        assert_eq!(find(&report, Check::StaleBranches).status, Status::Pass);
    }

    #[test]
    fn the_default_branch_is_never_the_one_waiting_for_review() {
        let mut repo = repo();
        repo.default_branch = Some("main".into());
        repo.branch_list = Some(vec![branch("main", "2026-01-01T00:00:00Z", 0, false)]);
        let report = evaluate(&snapshot(vec![repo]));
        assert_eq!(find(&report, Check::StaleBranches).status, Status::Pass);
    }

    #[test]
    fn a_merged_branch_nobody_deleted_is_not_waiting_for_review() {
        let mut repo = repo();
        repo.branch_list = Some(vec![branch(
            "feature/landed",
            "2026-11-01T00:00:00Z",
            1,
            true,
        )]);
        let report = evaluate(&snapshot(vec![repo]));
        assert_eq!(find(&report, Check::StaleBranches).status, Status::Pass);
    }

    #[test]
    fn an_unreadable_commit_date_is_not_evidence_of_staleness() {
        let mut repo = repo();
        repo.branch_list = Some(vec![branch("odd", "not a date", 0, false)]);
        let report = evaluate(&snapshot(vec![repo]));
        assert_eq!(find(&report, Check::StaleBranches).status, Status::Pass);
    }

    #[test]
    fn a_snapshot_without_a_branch_list_is_unknown() {
        let report = evaluate(&snapshot(vec![repo()]));
        assert!(find(&report, Check::StaleBranches).status.is_unknown());
    }

    #[test]
    fn a_library_is_not_asked_for_metrics_or_alerts() {
        // Nothing listens, so there is nothing to scrape and nothing to page
        // on. Failing here would drown the services in the libraries.
        let mut library = repo();
        with_service(&mut library, &[], &[], &[]);
        let report = evaluate(&snapshot(vec![library]));
        assert_eq!(find(&report, Check::Metrics).status, Status::Pass);
        assert_eq!(find(&report, Check::Alerts).status, Status::Pass);
    }

    #[test]
    fn a_service_that_serves_without_a_metrics_client_fails_and_says_which_server() {
        let mut server = repo();
        with_service(&mut server, &["axum"], &[], &[]);
        let report = evaluate(&snapshot(vec![server]));
        match &find(&report, Check::Metrics).status {
            Status::Fail { detail } => assert!(detail.contains("axum"), "{detail}"),
            other => panic!("expected a failure, got {other:?}"),
        }
        // Nothing is exported, so there is nothing to alert on yet.
        assert_eq!(find(&report, Check::Alerts).status, Status::Pass);
    }

    #[test]
    fn exporting_metrics_without_rules_fails_and_names_where_the_file_goes() {
        let mut server = repo();
        with_service(&mut server, &["axum"], &["prometheus"], &[]);
        let report = evaluate(&snapshot(vec![server]));
        assert_eq!(find(&report, Check::Metrics).status, Status::Pass);
        match &find(&report, Check::Alerts).status {
            Status::Fail { detail } => {
                assert!(detail.contains("prometheus"), "{detail}");
                assert!(detail.contains("deploy/prometheus/alerts.yml"), "{detail}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_scrapable_service_with_rules_passes_both() {
        let mut server = repo();
        with_service(
            &mut server,
            &["axum"],
            &["prometheus"],
            &["deploy/prometheus/alerts.yml"],
        );
        let report = evaluate(&snapshot(vec![server]));
        assert_eq!(find(&report, Check::Metrics).status, Status::Pass);
        assert_eq!(find(&report, Check::Alerts).status, Status::Pass);
    }

    #[test]
    fn a_snapshot_that_never_read_a_manifest_is_unknown_not_clean() {
        let report = evaluate(&snapshot(vec![repo()]));
        assert!(find(&report, Check::Metrics).status.is_unknown());
        assert!(find(&report, Check::Alerts).status.is_unknown());
    }

    #[test]
    fn an_instruction_file_that_still_says_bd_fails_and_names_where() {
        let mut stale = repo();
        stale.tracker = Some(crate::Tracker {
            files: vec!["AGENTS.md".into()],
            stale: crate::tracker::scan("AGENTS.md", "Run `bd prime` first."),
            unread: Vec::new(),
        });
        let report = evaluate(&snapshot(vec![stale]));
        match &find(&report, Check::Tracker).status {
            Status::Fail { detail } => {
                assert!(detail.contains("AGENTS.md"), "{detail}");
                assert!(detail.contains("`bd`"), "{detail}");
                assert!(detail.contains("marbles"), "{detail}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn instruction_files_that_say_mb_pass() {
        let mut clean = repo();
        clean.tracker = Some(crate::Tracker {
            files: vec!["AGENTS.md".into()],
            stale: Vec::new(),
            unread: Vec::new(),
        });
        let report = evaluate(&snapshot(vec![clean]));
        assert_eq!(find(&report, Check::Tracker).status, Status::Pass);
    }

    #[test]
    fn a_repository_with_no_instructions_is_not_a_failure() {
        // The check asks whether the instructions that exist are stale. It
        // deliberately does not ask a second question — "should there be
        // instructions at all" — because one failure with two fixes is a
        // failure nobody knows how to close.
        let mut bare = repo();
        bare.tracker = Some(crate::Tracker::default());
        let report = evaluate(&snapshot(vec![bare]));
        assert_eq!(find(&report, Check::Tracker).status, Status::Pass);
    }

    #[test]
    fn instructions_the_forge_would_not_return_are_unknown_not_clean() {
        let mut hidden = repo();
        hidden.tracker = Some(crate::Tracker {
            files: Vec::new(),
            stale: Vec::new(),
            unread: vec!["AGENTS.md".into()],
        });
        let report = evaluate(&snapshot(vec![hidden]));
        assert!(find(&report, Check::Tracker).status.is_unknown());
    }

    #[test]
    fn a_snapshot_collected_before_the_check_existed_is_unknown() {
        let report = evaluate(&snapshot(vec![repo()]));
        assert!(find(&report, Check::Tracker).status.is_unknown());
    }

    #[test]
    fn a_red_default_branch_fails_and_an_unconcluded_run_does_not() {
        let mut red = repo();
        red.latest_run = Some(Run {
            workflow: "ci".into(),
            conclusion: "failure".into(),
            at: None,
        });
        let report = evaluate(&snapshot(vec![red]));
        assert!(find(&report, Check::CiGreen).status.is_fail());

        let mut running = repo();
        running.latest_run = Some(Run {
            workflow: "ci".into(),
            conclusion: String::new(),
            at: None,
        });
        let report = evaluate(&snapshot(vec![running]));
        assert!(find(&report, Check::CiGreen).status.is_unknown());
    }

    #[test]
    fn staleness_needs_a_reference_time_and_says_so_without_one() {
        let mut snapshot = snapshot(vec![repo()]);
        snapshot.collected_at = None;
        let report = evaluate(&snapshot);
        assert!(find(&report, Check::Active).status.is_unknown());

        snapshot.collected_at = Some("2027-03-01T00:00:00Z".into());
        let report = evaluate(&snapshot);
        assert!(find(&report, Check::Active).status.is_fail());
    }

    #[test]
    fn branch_and_pull_counts_are_thresholds_not_booleans() {
        let mut busy = repo();
        busy.branches = 51;
        busy.open_pulls = 11;
        let report = evaluate(&snapshot(vec![busy]));
        assert!(find(&report, Check::Branches).status.is_fail());
        assert!(find(&report, Check::OpenPulls).status.is_fail());
    }

    #[test]
    fn a_broken_link_fails_and_a_redirected_one_passes() {
        let mut linked = repo();
        linked.links = Some(vec![
            Link {
                url: "https://a.example".into(),
                status: Some(301),
                error: None,
            },
            Link {
                url: "https://b.example".into(),
                status: Some(404),
                error: None,
            },
        ]);
        let report = evaluate(&snapshot(vec![linked]));
        match &find(&report, Check::DocsLinks).status {
            Status::Fail { detail } => assert!(detail.contains("1 of 2"), "{detail}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn an_exemption_is_visible_rather_than_silent() {
        let mut exempt = repo();
        exempt.license = None;
        exempt
            .exempt
            .insert("license".into(), "proprietary, see legal".into());
        let report = evaluate(&snapshot(vec![exempt]));
        assert_eq!(
            find(&report, Check::License).status,
            Status::Exempt {
                reason: "proprietary, see legal".into()
            }
        );
    }

    #[test]
    fn archived_and_forked_repositories_are_counted_not_scored() {
        let mut archived = repo();
        archived.archived = true;
        let mut fork = repo();
        fork.name = "forked".into();
        fork.fork = true;
        let report = evaluate(&snapshot(vec![archived, fork]));
        assert!(report.repos.is_empty());
        assert_eq!(report.skipped.len(), 2);
        assert_eq!(report.skipped[0].reason, SkipReason::Archived);
        assert_eq!(report.skipped[1].reason, SkipReason::Fork);
    }

    #[test]
    fn the_policy_can_widen_the_scope() {
        let mut fork = repo();
        fork.fork = true;
        let mut snapshot = snapshot(vec![fork]);
        snapshot.policy.check_forks = true;
        let report = evaluate(&snapshot);
        assert_eq!(report.repos.len(), 1);
        assert!(report.skipped.is_empty());
    }

    fn readme(bytes: u32, badges: &[(&str, &str)]) -> Readme {
        Readme {
            present: true,
            bytes,
            badges: badges
                .iter()
                .map(|(label, url)| Badge {
                    label: (*label).into(),
                    url: (*url).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_readme_nobody_read_is_unknown_and_one_that_is_missing_is_a_failure() {
        let mut unread = repo();
        unread.readme = None;
        let report = evaluate(&snapshot(vec![unread]));
        assert!(find(&report, Check::Readme).status.is_unknown());

        let mut absent = repo();
        absent.readme = Some(Readme {
            present: false,
            bytes: 0,
            badges: vec![],
        });
        let report = evaluate(&snapshot(vec![absent]));
        assert!(find(&report, Check::Readme).status.is_fail());
    }

    #[test]
    fn a_stub_readme_fails_where_a_document_passes() {
        let mut stub = repo();
        stub.readme = Some(readme(15, &[]));
        let report = evaluate(&snapshot(vec![stub]));
        assert!(find(&report, Check::Readme).status.is_fail());

        let mut document = repo();
        document.readme = Some(readme(4096, &[]));
        let report = evaluate(&snapshot(vec![document]));
        assert!(find(&report, Check::Readme).status.is_pass());
    }

    #[test]
    fn a_repository_with_no_readme_does_not_fail_twice() {
        let mut absent = repo();
        absent.readme = Some(Readme {
            present: false,
            bytes: 0,
            badges: vec![],
        });
        let report = evaluate(&snapshot(vec![absent]));
        assert!(find(&report, Check::Readme).status.is_fail());
        assert!(
            find(&report, Check::Badges).status.is_unknown(),
            "the readme check owns that absence"
        );
    }

    #[test]
    fn a_readme_with_no_ci_badge_fails_even_when_it_has_others() {
        let mut unbadged = repo();
        unbadged.readme = Some(readme(
            4096,
            &[
                ("crates", "https://img.shields.io/crates/v/bob.svg"),
                ("docs", "https://docs.rs/bob/badge.svg"),
            ],
        ));
        let report = evaluate(&snapshot(vec![unbadged]));
        match &find(&report, Check::Badges).status {
            Status::Fail { detail } => {
                assert!(detail.contains("no CI status badge"), "{detail}");
                assert!(detail.contains("2 other image"), "{detail}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_badge_pointing_at_a_deleted_workflow_fails() {
        let mut stale = repo(); // the fixture has ci.yml only
        stale.readme = Some(readme(
            4096,
            &[(
                "tests",
                "https://github.com/fpl/bob/actions/workflows/tests.yml/badge.svg",
            )],
        ));
        let report = evaluate(&snapshot(vec![stale]));
        match &find(&report, Check::Badges).status {
            Status::Fail { detail } => {
                assert!(detail.contains("tests.yml"), "{detail}");
                assert!(detail.contains("not in .github/workflows"), "{detail}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_badge_from_a_service_that_is_gone_fails_before_the_others() {
        let mut dead = repo();
        dead.readme = Some(readme(
            4096,
            &[
                (
                    "ci",
                    "https://github.com/fpl/bob/actions/workflows/ci.yml/badge.svg",
                ),
                ("build", "https://travis-ci.org/fpl/bob.svg?branch=main"),
            ],
        ));
        let report = evaluate(&snapshot(vec![dead]));
        match &find(&report, Check::Badges).status {
            Status::Fail { detail } => {
                assert!(
                    detail.contains("no longer exists") && detail.contains("travis-ci.org"),
                    "{detail}"
                );
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_badge_pointing_at_a_workflow_that_exists_passes() {
        let mut good = repo();
        good.readme = Some(readme(
            4096,
            &[(
                "ci",
                "https://github.com/fpl/bob/actions/workflows/ci.yml/badge.svg?branch=main",
            )],
        ));
        let report = evaluate(&snapshot(vec![good]));
        assert_eq!(find(&report, Check::Badges).status, Status::Pass);
    }

    #[test]
    fn repositories_come_out_sorted_so_two_reports_diff_cleanly() {
        let mut second = repo();
        second.name = "aaa".into();
        let report = evaluate(&snapshot(vec![repo(), second]));
        assert_eq!(report.repos[0].slug(), "fpl/aaa");
        assert_eq!(report.repos[1].slug(), "fpl/bob");
    }
}
