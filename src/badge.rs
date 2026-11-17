//! What a README's badges are, and whether they mean anything.
//!
//! A badge is a promise that something is being watched. A badge pointing at a
//! workflow file that was deleted six months ago is not a promise, it is a
//! broken image — and it is the most common way a README starts lying, because
//! nothing in the repository notices when the workflow it advertises goes away.
//!
//! Extraction is here because it is text parsing; classification is here because
//! it is judgement and the two are tested together. Which badges count as a
//! failure is [`crate::check`], which reads the classified kinds rather than the
//! URLs, so a policy about badges never has to know what a URL looks like.

use serde::{Deserialize, Serialize};

/// One image found in a README.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Badge {
    /// The markdown alt text, or `<img>` alt when there is one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub url: String,
}

/// Every image a README embeds, in the order they appear.
///
/// Markdown images (`![alt](url)`) and `<img src=...>` both count: a README that
/// centres its badge block with HTML still carries badges. Ordinary links do
/// not, because a link is not an image and calling one a badge would make the
/// check pass on a repository that has none.
#[must_use]
pub fn extract(markdown: &str) -> Vec<Badge> {
    let mut badges = Vec::new();
    let bytes = markdown.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        // Markdown: `![alt](url)`.
        if bytes[index] == b'!' && bytes.get(index + 1) == Some(&b'[') {
            if let Some((label, url, next)) = markdown_image(markdown, index + 2) {
                badges.push(Badge { label, url });
                index = next;
                continue;
            }
        }
        // HTML: `<img src="url">`.
        if bytes[index] == b'<' {
            if let Some((alt, url, next)) = html_image(markdown, index + 1) {
                badges.push(Badge { label: alt, url });
                index = next;
                continue;
            }
        }
        index += 1;
    }
    badges
}

fn markdown_image(text: &str, open: usize) -> Option<(String, String, usize)> {
    let rest = text.get(open..)?;
    let close = rest.find(']')?;
    let label = rest.get(..close)?.to_string();
    let after = rest.get(close + 1..)?;
    let after = after.strip_prefix('(')?;
    let end = after.find(')')?;
    let url = clean_url(&after[..end])?;
    Some((label, url, open + close + 1 + end + 2))
}

fn html_image(text: &str, from: usize) -> Option<(String, String, usize)> {
    let rest = text.get(from..)?;
    if !rest.starts_with("img") {
        return None;
    }
    let end = rest.find('>')?;
    let tag = rest.get(..end)?;
    let url = attribute(tag, "src")?;
    let alt = attribute(tag, "alt").unwrap_or_default();
    Some((alt, url, from + end + 1))
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let start = tag.find(&needle)? + needle.len();
    let rest = tag.get(start..)?;
    let rest = rest.strip_prefix('"').or_else(|| rest.strip_prefix('\''))?;
    let end = rest.find(['"', '\''])?;
    clean_url(rest.get(..end)?)
}

/// A markdown destination may carry a title (`(url "title")`) and a query
/// string; the URL is what the badge points at.
fn clean_url(raw: &str) -> Option<String> {
    let url = raw.split_whitespace().next()?.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    Some(url.to_string())
}

/// What a badge is watching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeKind {
    /// A GitHub Actions status badge. `workflow` is the file name when the URL
    /// names one, and the workflow's *name* when it is the older form
    /// (`/workflows/Rust/badge.svg`), which cannot be checked against a file
    /// listing.
    Actions {
        workflow: Option<String>,
        legacy: bool,
    },
    /// A live CI service that is not Actions.
    Ci { host: String },
    /// A service that has been shut down. The badge is a 404 with a picture.
    Dead { host: String },
    /// Anything else: crates, docs, licence, downloads, coverage.
    Other,
}

/// Classify one badge URL.
#[must_use]
pub fn classify(url: &str) -> BadgeKind {
    let stripped = strip_scheme(url);
    let path = stripped.split(['?', '#']).next().unwrap_or(stripped);
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let host = segments.first().copied().unwrap_or_default();

    if is_dead(host) {
        return BadgeKind::Dead {
            host: host.to_string(),
        };
    }
    if let Some(workflow) = actions_workflow(&segments) {
        return workflow;
    }
    if let Some(host) = live_ci(host) {
        return BadgeKind::Ci { host };
    }
    BadgeKind::Other
}

fn strip_scheme(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// Services that no longer exist. A badge from one of these is not a weak
/// signal, it is a broken image on the front page of the repository.
fn is_dead(host: &str) -> bool {
    matches!(
        host,
        "travis-ci.org" | "travis-ci.com" | "api.travis-ci.com" | "www.travis-ci.org"
    )
}

fn live_ci(host: &str) -> Option<String> {
    let trimmed = host.strip_prefix("www.").unwrap_or(host);
    let live = [
        "circleci.com",
        "app.circleci.com",
        "buildkite.com",
        "gitlab.com",
        "codecov.io",
    ];
    live.contains(&trimmed).then(|| trimmed.to_string())
}

/// `github.com/{owner}/{repo}/actions/workflows/{file}/badge.svg`, the
/// `.../workflows/{name}/badge.svg` form it replaced, and the shields.io mirror
/// of the first.
fn actions_workflow(segments: &[&str]) -> Option<BadgeKind> {
    if segments.first() == Some(&"github.com") {
        // github.com / owner / repo / actions / workflows / <file> / badge.svg
        if segments.len() >= 7
            && segments.get(3) == Some(&"actions")
            && segments.get(4) == Some(&"workflows")
        {
            let workflow = segments.get(5)?;
            return Some(BadgeKind::Actions {
                workflow: Some((*workflow).to_string()),
                legacy: false,
            });
        }
        // github.com / owner / repo / workflows / <name> / badge.svg
        if segments.len() >= 6 && segments.get(3) == Some(&"workflows") {
            let name = segments.get(4)?;
            return Some(BadgeKind::Actions {
                workflow: Some((*name).to_string()),
                legacy: true,
            });
        }
    }
    // img.shields.io/github/actions/workflow/status/{owner}/{repo}/{file}
    if segments.first() == Some(&"img.shields.io")
        && segments.len() >= 8
        && segments.get(1) == Some(&"github")
        && segments.get(2) == Some(&"actions")
        && segments.get(3) == Some(&"workflow")
        && segments.get(4) == Some(&"status")
    {
        let workflow = segments.get(7)?;
        return Some(BadgeKind::Actions {
            workflow: Some((*workflow).to_string()),
            legacy: false,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Badge, BadgeKind, classify, extract};

    fn urls(markdown: &str) -> Vec<String> {
        extract(markdown)
            .into_iter()
            .map(|badge| badge.url)
            .collect()
    }

    #[test]
    fn reads_a_badge_block_with_alt_text() {
        let found = extract(
            "[![ci](https://github.com/o/r/actions/workflows/ci.yml/badge.svg)](https://github.com/o/r/actions)",
        );
        assert_eq!(
            found,
            vec![Badge {
                label: "ci".into(),
                url: "https://github.com/o/r/actions/workflows/ci.yml/badge.svg".into()
            }]
        );
    }

    #[test]
    fn plain_links_are_not_badges() {
        assert!(urls("[the docs](https://example.com/docs)").is_empty());
    }

    #[test]
    fn html_images_count_because_badge_blocks_are_often_centred() {
        assert_eq!(
            urls(
                r#"<p align="center"><img src="https://img.shields.io/badge/build-passing.svg" alt="build"></p>"#
            ),
            vec!["https://img.shields.io/badge/build-passing.svg".to_string()]
        );
    }

    #[test]
    fn a_title_after_the_url_does_not_become_part_of_it() {
        assert_eq!(
            urls(r#"![ci](https://github.com/o/r/actions/workflows/ci.yml/badge.svg "status")"#),
            vec!["https://github.com/o/r/actions/workflows/ci.yml/badge.svg".to_string()]
        );
    }

    #[test]
    fn classifies_the_forms_a_real_fleet_uses() {
        assert_eq!(
            classify("https://github.com/o/r/actions/workflows/ci.yml/badge.svg?branch=main"),
            BadgeKind::Actions {
                workflow: Some("ci.yml".into()),
                legacy: false
            }
        );
        assert_eq!(
            classify("https://github.com/o/r/workflows/Rust/badge.svg"),
            BadgeKind::Actions {
                workflow: Some("Rust".into()),
                legacy: true
            }
        );
        assert_eq!(
            classify("https://img.shields.io/github/actions/workflow/status/o/r/ci.yml"),
            BadgeKind::Actions {
                workflow: Some("ci.yml".into()),
                legacy: false
            }
        );
        assert_eq!(
            classify("https://travis-ci.org/o/r.svg?branch=master"),
            BadgeKind::Dead {
                host: "travis-ci.org".into()
            }
        );
        assert_eq!(
            classify("https://gitlab.com/o/r/badges/master/pipeline.svg"),
            BadgeKind::Ci {
                host: "gitlab.com".into()
            }
        );
        assert_eq!(classify("https://docs.rs/r/badge.svg"), BadgeKind::Other);
        assert_eq!(
            classify("https://img.shields.io/crates/v/r.svg"),
            BadgeKind::Other
        );
    }
}
