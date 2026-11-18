//! The report as messages: one event per repository that fails something.
//!
//! A digest is what a person reads and a report is what a document keeps. A bus
//! needs the third thing: one small message per subject, so a subscriber that
//! cares about `mycelium-linux/canvas` does not have to receive the fleet and
//! filter it. That is why these are lines rather than one array — a line is the
//! unit `unibus-publish` takes, and one publisher per line is one message on the
//! bus with its own id.
//!
//! Every event is level-triggered, not edge-triggered: it says what is failing
//! now, not what changed since last night. A subscriber that was down for the
//! scan still learns the truth on the next one, which is the property a
//! notification system needs and a changelog does not have. Working out the
//! transition is the subscriber's business, and the report file is there for it.

use serde::{Deserialize, Serialize};

use crate::check::Severity;
use crate::report::Report;

/// One failing check, with the sentence that says what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub check: String,
    pub severity: Severity,
    pub detail: String,
}

/// One repository, as one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// `owner/name`, the form every message in the fleet uses.
    pub repo: String,
    pub org: String,
    pub name: String,
    /// When the collector looked, carried per event so a subscriber that missed
    /// the scan-level message still knows how old this is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    /// What failed, worst first. Never empty: a repository with nothing failing
    /// is not an event.
    pub failing: Vec<Failure>,
    /// Checks that had no evidence. Not the same as passing, and not a failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<String>,
}

impl Event {
    /// The worst finding in this event.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.failing
            .iter()
            .map(|failure| failure.severity)
            .max()
            .unwrap_or(Severity::Advisory)
    }
}

/// Every repository that fails something, in report order (which is slug order).
///
/// A repository with only unknown checks is not an event: nobody can act on
/// "the evidence did not arrive" by opening the repository.
#[must_use]
pub fn events(report: &Report) -> Vec<Event> {
    let mut events = Vec::new();
    for repo in &report.repos {
        let mut failing = Vec::new();
        let mut unknown = Vec::new();
        for finding in &repo.findings {
            match &finding.status {
                crate::Status::Fail { detail } => failing.push(Failure {
                    check: finding.check.id().to_owned(),
                    severity: finding.severity,
                    detail: detail.clone(),
                }),
                crate::Status::Unknown { .. } => {
                    unknown.push(finding.check.id().to_owned());
                }
                crate::Status::Pass | crate::Status::Exempt { .. } => {}
            }
        }
        if failing.is_empty() {
            continue;
        }
        events.push(Event {
            repo: repo.slug(),
            org: repo.org.clone(),
            name: repo.name.clone(),
            checked_at: report.collected_at.clone(),
            failing,
            unknown,
        });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::events;
    use crate::{Report, Snapshot, evaluate};

    fn report_for(names: &[(&str, Option<&str>)]) -> Report {
        let repos = names
            .iter()
            .map(|(name, license)| {
                let licensed = license.is_some();
                crate::RepoState {
                    org: "fpl".into(),
                    name: (*name).into(),
                    license: license.map(str::to_string),
                    pushed_at: Some("2026-11-17T00:00:00Z".into()),
                    // A repository with a licence is the one that is otherwise
                    // in order, so the "nothing fails" case has something to be
                    // clean with rather than passing by accident of omission.
                    workflows: if licensed {
                        vec!["ci.yml".into()]
                    } else {
                        Vec::new()
                    },
                    releases: u32::from(licensed),
                    description: licensed.then(|| "a build plane".to_string()),
                    readme: licensed.then(|| crate::Readme {
                        present: true,
                        bytes: 1024,
                        badges: vec![crate::Badge {
                            label: "ci".into(),
                            url: "https://github.com/fpl/bob/actions/workflows/ci.yml/badge.svg"
                                .into(),
                        }],
                    }),
                    ..crate::RepoState::default()
                }
            })
            .collect();
        evaluate(&Snapshot {
            schema: crate::SCHEMA,
            collected_at: Some("2026-11-17T12:00:00Z".into()),
            policy: crate::Policy::default(),
            repos,
        })
    }

    #[test]
    fn a_repository_that_fails_nothing_is_not_an_event() {
        let report = report_for(&[("clean", Some("MIT"))]);
        assert!(events(&report).is_empty());
    }

    #[test]
    fn a_failing_repository_carries_its_checks_and_the_collection_time() {
        let report = report_for(&[("bare", None)]);
        let events = events(&report);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.repo, "fpl/bare");
        assert_eq!(event.checked_at.as_deref(), Some("2026-11-17T12:00:00Z"));
        let checks: Vec<&str> = event.failing.iter().map(|f| f.check.as_str()).collect();
        assert!(checks.contains(&"license"), "{checks:?}");
        assert_eq!(event.severity(), crate::Severity::Critical);
    }

    #[test]
    fn one_line_per_repository_and_the_order_is_the_reports() {
        let report = report_for(&[("b", None), ("a", None)]);
        let slugs: Vec<String> = events(&report).into_iter().map(|e| e.repo).collect();
        assert_eq!(slugs, vec!["fpl/a", "fpl/b"]);
    }

    #[test]
    fn a_serialized_event_is_one_line_of_json_under_the_bus_limit() {
        let report = report_for(&[("bare", None)]);
        let event = &events(&report)[0];
        let line = serde_json::to_string(event).expect("an event serializes");
        assert!(!line.contains('\n'), "{line}");
        assert!(line.len() < 64 * 1024, "{} bytes", line.len());
        let back: super::Event = serde_json::from_str(&line).expect("and reads back");
        assert_eq!(&back, event);
    }

    #[test]
    fn a_repository_with_only_unknown_evidence_is_left_alone() {
        // `ci-green` and `docs-links` have no evidence in a hand-written
        // snapshot, and "the collector did not say" is not something a person
        // can fix by opening the repository.
        let report = report_for(&[("clean", Some("MIT"))]);
        let unknown = report.repos[0]
            .findings
            .iter()
            .filter(|finding| finding.status.is_unknown())
            .count();
        assert!(
            unknown > 0,
            "the fixture relies on these having no evidence"
        );
        assert!(events(&report).is_empty(), "unknown alone is not an event");
    }
}
