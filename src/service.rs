//! Whether a repository can be watched: does it serve HTTP, does it export
//! Prometheus metrics, and does it carry the alert rules for what it exports.
//!
//! The evidence is deliberately narrow. It is the dependency manifests a
//! repository already has and the rule files Prometheus reads — never its source
//! code. A check that greps a handler for `/metrics` passes the repository that
//! registers the route under a variable and fails the one that builds it in a
//! macro, and both answers would be wrong in a way nobody can see from the
//! report. A manifest saying `axum` and a rules file saying `alert:` are claims
//! the repository made about itself.
//!
//! The cost of being narrow is that the check cannot see a metrics endpoint
//! somebody rolled by hand. That is the right trade for a fleet check: it fails
//! repositories that are definitely unwatched and stays quiet about the rest.

use serde::{Deserialize, Serialize};

/// What a repository runs, and whether that thing can be watched.
///
/// `None` on [`crate::RepoState`] means nobody looked, which is different from a
/// repository that was read and is not a service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    /// The HTTP server dependencies found, by name. Empty means this is a
    /// library or a command, and there is nothing to scrape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http: Vec<String>,
    /// The metrics client dependencies found, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<String>,
    /// The alert-rule files found, by path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

impl Service {
    /// Whether anything here is meant to be scraped.
    #[must_use]
    pub fn serves(&self) -> bool {
        !self.http.is_empty() || !self.metrics.is_empty()
    }
}

/// Dependencies that mean "this process listens on a socket".
///
/// One list for every ecosystem, because the question is the same in all of
/// them and a table per language is four tables that drift. The names are
/// distinct enough that a whole-word match in a manifest is honest.
pub const HTTP_SERVERS: &[&str] = &[
    // Rust
    "axum",
    "actix-web",
    "warp",
    "rocket",
    "poem",
    "tide",
    "salvo",
    "hyper",
    // Go
    "gin-gonic/gin",
    "labstack/echo",
    "go-chi/chi",
    "gorilla/mux",
    "gofiber/fiber",
    // Node
    "express",
    "fastify",
    "koa",
    "@hapi/hapi",
    "restify",
    // Python
    "fastapi",
    "flask",
    "django",
    "starlette",
    "aiohttp",
    "tornado",
    "uvicorn",
];

/// Dependencies that mean "this process exports Prometheus metrics".
pub const METRICS_CLIENTS: &[&str] = &[
    "prometheus",
    "prometheus-client",
    "prometheus_client",
    "prom-client",
    "metrics-exporter-prometheus",
    "axum-prometheus",
    "actix-web-prom",
    "opentelemetry-prometheus",
    "client_golang",
    "opentelemetry",
];

/// The files a repository might carry its alert rules in.
///
/// A short list on purpose, and the remedy names the first one: a check that
/// accepts any YAML anywhere accepts a repository nobody can alert on, and a
/// list of every layout the fleet has ever used is a list that rots.
pub const ALERT_PATHS: &[&str] = &[
    "deploy/prometheus/alerts.yml",
    "prometheus/alerts.yml",
    "monitoring/alerts.yml",
    "alerts.yml",
];

/// Whether `text` names `token` as a whole word.
///
/// Cargo names a dependency `axum` and a description says "a metrics thing";
/// whole words keep the second from counting as the first. Not bulletproof —
/// `description = "http server"` still matches nothing, but a `[package] name`
/// of `metrics` would — and the remedy asks a person to confirm rather than
/// pretending to be certain.
fn mentions(text: &str, token: &str) -> bool {
    let haystack = text.as_bytes();
    let needle = token.as_bytes();
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .any(|(index, window)| {
            window == needle
                && !index
                    .checked_sub(1)
                    .and_then(|before| haystack.get(before))
                    .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
                && !haystack
                    .get(index + needle.len())
                    .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
        })
}

/// Read one manifest and report which known dependencies it names.
fn named(text: &str, vocabulary: &[&str]) -> Vec<String> {
    vocabulary
        .iter()
        .filter(|token| mentions(text, token))
        .map(|token| (*token).to_string())
        .collect()
}

/// Build the service picture from the manifests and rule files a page carried.
#[must_use]
pub fn classify(manifests: &[(String, String)], rules: &[(String, String)]) -> Service {
    let mut service = Service::default();
    for (_, text) in manifests {
        for name in named(text, HTTP_SERVERS) {
            if !service.http.contains(&name) {
                service.http.push(name);
            }
        }
        for name in named(text, METRICS_CLIENTS) {
            if !service.metrics.contains(&name) {
                service.metrics.push(name);
            }
        }
    }
    service.http.sort();
    service.metrics.sort();
    for (path, text) in rules {
        // `alert:` is the key a rule has; `groups:` is the file that holds them.
        if text.contains("alert:") || text.contains("groups:") {
            service.alerts.push(path.clone());
        }
    }
    service.alerts.sort();
    service
}

#[cfg(test)]
mod tests {
    use super::{ALERT_PATHS, classify};

    fn manifests(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(path, text)| ((*path).to_string(), (*text).to_string()))
            .collect()
    }

    #[test]
    fn a_rust_service_with_metrics_is_recognised() {
        let service = classify(
            &manifests(&[(
                "Cargo.toml",
                "[dependencies]\naxum = \"0.7\"\nprometheus = \"0.13\"\n",
            )]),
            &[],
        );
        assert_eq!(service.http, vec!["axum"]);
        assert_eq!(service.metrics, vec!["prometheus"]);
        assert!(service.serves());
    }

    #[test]
    fn a_library_is_not_a_service() {
        let service = classify(
            &manifests(&[("Cargo.toml", "[package]\nname = \"serde-helper\"\n")]),
            &[],
        );
        assert!(!service.serves(), "{service:?}");
    }

    #[test]
    fn a_description_that_merely_says_metrics_does_not_count() {
        // `metrics` as a dependency is a client; `metrics` in prose is a word in
        // a sentence. Only the second one is here, and it must not pass.
        let service = classify(
            &manifests(&[(
                "Cargo.toml",
                "[package]\nname = \"sensor\"\ndescription = \"sensors, matrixes and metrics\"\n",
            )]),
            &[],
        );
        assert!(service.metrics.is_empty(), "{service:?}");
    }

    #[test]
    fn go_and_node_and_python_manifests_are_read_the_same_way() {
        let service = classify(
            &manifests(&[
                (
                    "go.mod",
                    "require (\n\tgithub.com/prometheus/client_golang v1.19.0\n)",
                ),
                ("package.json", r#"{"dependencies":{"express":"4.19.2"}}"#),
                (
                    "pyproject.toml",
                    "[project]\ndependencies = [\"fastapi\"]\n",
                ),
            ]),
            &[],
        );
        assert!(service.http.contains(&"express".to_string()));
        assert!(service.http.contains(&"fastapi".to_string()));
        assert!(service.metrics.contains(&"client_golang".to_string()));
    }

    #[test]
    fn a_rules_file_has_to_hold_rules() {
        let rules = vec![
            (
                "prometheus/alerts.yml".to_string(),
                "groups:\n  - name: x\n".to_string(),
            ),
            (
                "deploy/prometheus/alerts.yml".to_string(),
                "name: nothing\n".to_string(),
            ),
        ];
        let service = classify(&[], &rules);
        assert_eq!(service.alerts, vec!["prometheus/alerts.yml"]);
    }

    #[test]
    fn the_remedys_first_path_is_one_the_check_reads() {
        // The template the remedy prints has to land where the scanner looks,
        // or the finding can never be closed by following the finding.
        assert!(ALERT_PATHS.contains(&"deploy/prometheus/alerts.yml"));
        assert!(ALERT_PATHS.contains(&"alerts.yml"));
    }
}
