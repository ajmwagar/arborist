//! The one paragraph arborist puts on a bus.
//!
//! A report is a document: every repository, every finding, the policy it was
//! read under. That is the right thing to keep and the wrong thing to send. A
//! phone notification, a chat message, or a note in a log needs the answer, not
//! the evidence.
//!
//! So this is the second rendering of the same [`Report`]: counts, the checks
//! that failed, and the repositories worth opening first. It computes nothing
//! the report does not already say — it selects.
//!
//! The body is deliberately small and flat. `unibus` caps a control body at
//! 64 KiB and refuses one that is not an object, and both rules are good ones:
//! a notification that has to be paged is not a notification.

use serde::{Deserialize, Serialize};

use crate::check::Severity;
use crate::report::Report;

/// One repository worth opening, with the checks that failed on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedRepo {
    pub repo: String,
    /// The failing check ids, worst first.
    pub checks: Vec<String>,
    pub severity: Severity,
}

/// One check's slice of the fleet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckSlice {
    pub check: String,
    pub severity: Severity,
    pub fail: usize,
    pub unknown: usize,
}

/// What goes on the bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digest {
    pub orgs: Vec<String>,
    pub checked: usize,
    pub skipped: usize,
    pub critical: usize,
    pub advisory: usize,
    pub clean: usize,
    pub unknown: usize,
    /// Whether the fleet is failing. The same answer the gate gives, stated
    /// once so a consumer does not have to re-derive it from the counts.
    pub failing: bool,
    /// One sentence, for a subject line or a notification title.
    pub headline: String,
    /// The repositories to open first, worst first.
    pub worst: Vec<NamedRepo>,
    /// Every check that failed somewhere, worst first.
    pub by_check: Vec<CheckSlice>,
}

/// How many repositories the digest names. Enough to start on, few enough to
/// read on a lock screen; the report is where the rest is.
pub const NAMED: usize = 10;

/// Select the bus-sized answer out of a report.
#[must_use]
pub fn digest(report: &Report) -> Digest {
    let summary = &report.summary;
    let failing = summary.critical + summary.advisory > 0;

    let mut by_check: Vec<CheckSlice> = report
        .summary
        .by_check
        .iter()
        .filter(|entry| entry.fail > 0)
        .map(|entry| CheckSlice {
            check: entry.check.id().to_owned(),
            severity: entry.severity,
            fail: entry.fail,
            unknown: entry.unknown,
        })
        .collect();
    by_check.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| right.fail.cmp(&left.fail))
            .then_with(|| left.check.cmp(&right.check))
    });

    let worst: Vec<NamedRepo> = report
        .worst(false)
        .into_iter()
        .take(NAMED)
        .map(|repo| {
            let mut severity = Severity::Advisory;
            let checks = repo
                .fails()
                .map(|finding| {
                    if finding.severity > severity {
                        severity = finding.severity;
                    }
                    finding.check.id().to_owned()
                })
                .collect();
            NamedRepo {
                repo: repo.slug(),
                checks,
                severity,
            }
        })
        .collect();

    Digest {
        orgs: report.orgs.clone(),
        checked: summary.checked,
        skipped: summary.skipped,
        critical: summary.critical,
        advisory: summary.advisory,
        clean: summary.clean,
        unknown: summary.unknown,
        failing,
        headline: headline(report, &by_check),
        worst,
        by_check,
    }
}

/// One sentence. A notification's title is the whole message for most readers,
/// so it carries the number that matters and the reasons in the order they
/// should be fixed.
fn headline(report: &Report, by_check: &[CheckSlice]) -> String {
    let summary = &report.summary;
    let total = summary.checked;
    let plural = |n: usize| if n == 1 { "" } else { "s" };

    if summary.critical == 0 && summary.advisory == 0 {
        return format!("all {total} repositories pass every check they have evidence for");
    }
    if summary.critical > 0 {
        let reasons: Vec<String> = by_check
            .iter()
            .filter(|entry| entry.severity == Severity::Critical)
            .take(3)
            .map(|entry| format!("{} {}", entry.check, entry.fail))
            .collect();
        return format!(
            "{} of {total} repositories fail a critical check: {}",
            summary.critical,
            reasons.join(", ")
        );
    }
    let reasons: Vec<String> = by_check
        .iter()
        .take(3)
        .map(|entry| format!("{} {}", entry.check, entry.fail))
        .collect();
    format!(
        "{} of {total} repositories {} advisory finding{}: {}",
        summary.advisory,
        if summary.advisory == 1 {
            "has an"
        } else {
            "have"
        },
        plural(summary.advisory),
        reasons.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::{NAMED, digest};
    use crate::check::Severity;
    use crate::policy::{Policy, evaluate};
    use crate::snapshot::{RepoState, Snapshot};

    /// A README with a badge that points at the workflow that exists. Without
    /// the badge the repository fails `badges`, which is the point of that
    /// check and would make every "clean" fixture here a lie.
    fn badged() -> crate::Readme {
        crate::Readme {
            present: true,
            bytes: 4096,
            badges: vec![crate::Badge {
                label: "ci".into(),
                url: "https://github.com/fpl/x/actions/workflows/ci.yml/badge.svg".into(),
            }],
        }
    }

    fn repo(name: &str, license: bool, ci: bool, readme: bool) -> RepoState {
        RepoState {
            org: "fpl".into(),
            name: name.into(),
            license: license.then(|| "MIT".to_owned()),
            workflows: if ci {
                vec!["ci.yml".into()]
            } else {
                Vec::new()
            },
            readme: Some(if readme {
                badged()
            } else {
                crate::Readme {
                    present: false,
                    bytes: 0,
                    badges: Vec::new(),
                }
            }),
            description: Some("x".into()),
            releases: 1,
            pushed_at: Some("2026-11-17T00:00:00Z".into()),
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

    #[test]
    fn a_broken_fleet_says_what_is_broken_and_how_much() {
        let report = evaluate(&snapshot(vec![
            repo("a", false, false, false),
            repo("b", false, true, true),
            repo("c", true, true, true),
        ]));
        let d = digest(&report);
        assert!(d.failing);
        assert_eq!(d.checked, 3);
        assert_eq!(d.critical, 2);
        assert_eq!(
            d.headline,
            "2 of 3 repositories fail a critical check: license 2, ci 1, readme 1"
        );
        assert_eq!(d.worst[0].repo, "fpl/a");
        assert_eq!(d.worst[0].severity, Severity::Critical);
        assert!(
            d.by_check[0].severity == Severity::Critical,
            "critical checks sort first: {:?}",
            d.by_check
        );
    }

    #[test]
    fn a_clean_fleet_says_so_instead_of_sending_an_empty_report() {
        let report = evaluate(&snapshot(vec![repo("a", true, true, true)]));
        let d = digest(&report);
        assert!(!d.failing, "{d:?}");
        assert_eq!(d.critical, 0);
        assert!(d.headline.starts_with("all 1 repositories"));
        assert!(d.worst.is_empty());
    }

    #[test]
    fn the_digest_names_at_most_the_handful_worth_opening() {
        let repos: Vec<RepoState> = (0..40)
            .map(|i| repo(&format!("r{i:02}"), false, false, false))
            .collect();
        let report = evaluate(&snapshot(repos));
        let d = digest(&report);
        assert_eq!(d.worst.len(), NAMED);
        assert_eq!(
            d.checked, 40,
            "the count is the whole fleet, not the sample"
        );
    }

    #[test]
    fn the_body_is_a_small_object_a_bus_will_take() {
        // `unibus` refuses a body that is not an object and one over 64 KiB.
        let repos: Vec<RepoState> = (0..300)
            .map(|i| repo(&format!("repo-number-{i:03}"), false, false, false))
            .collect();
        let report = evaluate(&snapshot(repos));
        let value = serde_json::to_value(digest(&report)).unwrap();
        assert!(value.is_object());
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(bytes.len() < 64 * 1024, "{} bytes", bytes.len());
        assert!(
            bytes.len() < 8 * 1024,
            "a notification should not be {} bytes",
            bytes.len()
        );
    }

    #[test]
    fn an_exempted_check_is_not_a_failure_and_not_in_the_digest() {
        let mut state = repo("a", true, true, true);
        state
            .exempt
            .insert("release".into(), "not shipped yet".into());
        let report = evaluate(&snapshot(vec![state]));
        let d = digest(&report);
        assert!(d.by_check.iter().all(|entry| entry.check != "release"));
    }

    #[test]
    fn the_digest_carries_the_unknowns_so_a_quiet_fleet_is_not_a_green_one() {
        let report = evaluate(&snapshot(vec![repo("a", true, true, true)]));
        let d = digest(&report);
        // ci-green and docs-links have no evidence in this fixture.
        assert_eq!(d.unknown, 1);
        assert!(!d.failing, "an unknown is not a failure: {d:?}");
    }

    #[test]
    fn findings_that_are_only_advisory_read_as_advisory() {
        let report = evaluate(&snapshot(vec![RepoState {
            org: "fpl".into(),
            name: "b".into(),
            license: Some("MIT".into()),
            workflows: vec!["ci.yml".into()],
            readme: Some(badged()),
            description: None,
            releases: 0,
            pushed_at: Some("2026-11-17T00:00:00Z".into()),
            ..RepoState::default()
        }]));
        let d = digest(&report);
        assert!(d.failing);
        assert!(d.headline.contains("advisory"), "{}", d.headline);
        assert!(d.headline.contains("has an advisory"), "{}", d.headline);
        assert!(!d.headline.contains("critical"), "{}", d.headline);
    }

    #[test]
    fn every_failing_check_appears_once_and_no_passing_one_does() {
        let report = evaluate(&snapshot(vec![repo("a", false, true, true)]));
        let d = digest(&report);
        assert_eq!(d.by_check.len(), 1);
        assert_eq!(d.by_check[0].check, "license");
    }
}
