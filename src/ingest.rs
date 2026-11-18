//! The forge adapter: GitHub's GraphQL pages in, a snapshot out.
//!
//! This lives in Rust rather than in the shell that calls it because a `jq`
//! program that maps forty fields is a program nobody reviews and nobody tests.
//! The pipeline is then one line — `gh api graphql --paginate | arborist ingest` —
//! and the mapping has a test per shape the forge actually returns.
//!
//! The input is a stream of JSON documents, not one document. `gh` emits one per
//! page, and collecting several owners is therefore several queries whose pages
//! concatenate. Every document is read until the stream ends.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::snapshot::{RepoState, SCHEMA, Snapshot};

/// A collector accumulates pages and hands back one snapshot.
#[derive(Debug, Default)]
pub struct Collector {
    repos: BTreeMap<String, RepoState>,
    orgs: Vec<String>,
}

impl Collector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read one GraphQL response, adding the repositories it names.
    ///
    /// Two shapes arrive here and both are the same fact: a page of one owner's
    /// repositories, and the single repository a pull-request gate asks about.
    /// A page that carries `errors` and no data is not an error — a fleet
    /// collection is allowed to lose one owner and still produce a report, and
    /// the pages that did arrive are the evidence for the rest.
    pub fn absorb(&mut self, document: &Value) -> usize {
        let Ok(parsed) = serde_json::from_value::<GraphQlDocument>(document.clone()) else {
            return 0;
        };
        let Some(data) = parsed.data else {
            return 0;
        };
        let mut added = 0;

        if let Some(owner) = data.repository_owner {
            if let (Some(login), Some(connection)) = (non_empty(owner.login), owner.repositories) {
                self.note_org(&login);
                for node in connection.nodes.into_iter().flatten() {
                    added += usize::from(self.offer(&login, node));
                }
            }
        }

        if let Some(node) = data.repository {
            let login = node
                .owner
                .as_ref()
                .and_then(|owner| non_empty(owner.login.clone()));
            if let Some(login) = login {
                self.note_org(&login);
                added += usize::from(self.offer(&login, node));
            }
        }

        added
    }

    fn note_org(&mut self, login: &str) {
        if !self.orgs.iter().any(|org| org == login) {
            self.orgs.push(login.to_string());
        }
    }

    fn offer(&mut self, login: &str, mut node: Node) -> bool {
        let Some(name) = non_empty(node.name.take()) else {
            return false;
        };
        self.repos
            .insert(format!("{login}/{name}"), repo_state(login, name, node));
        true
    }

    /// Read every document in a stream, returning how many repositories arrived.
    pub fn absorb_stream(&mut self, bytes: &[u8]) -> Result<usize, serde_json::Error> {
        let mut total = 0;
        let stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Value>();
        for document in stream {
            total += self.absorb(&document?);
        }
        Ok(total)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.repos.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
    }

    /// Owners seen, in the order their first page arrived.
    #[must_use]
    pub fn orgs(&self) -> &[String] {
        &self.orgs
    }

    #[must_use]
    pub fn finish(self, collected_at: Option<String>) -> Snapshot {
        let mut repos: Vec<RepoState> = self.repos.into_values().collect();
        repos.sort_by_key(|left| left.slug());
        Snapshot {
            schema: SCHEMA,
            collected_at,
            policy: crate::Policy::default(),
            repos,
        }
    }
}

/// One node, whichever document it came from, as the fact sheet the checks read.
fn repo_state(login: &str, name: String, node: Node) -> RepoState {
    RepoState {
        org: login.to_string(),
        name,
        archived: node.is_archived,
        fork: node.is_fork,
        private: node.is_private,
        pushed_at: non_empty(node.pushed_at),
        default_branch: node
            .default_branch_ref
            .and_then(|branch| non_empty(branch.name)),
        license: node
            .license_info
            .and_then(|license| non_empty(license.spdx_id)),
        description: node.description.filter(|text| !text.trim().is_empty()),
        releases: node.releases.map_or(0, |releases| releases.total_count),
        open_pulls: node.pull_requests.map_or(0, |pulls| pulls.total_count),
        branches: node.branches.map_or(0, |branches| branches.total_count),
        workflows: node
            .workflows
            .and_then(|tree| tree.entries)
            .map(|entries| {
                let mut names: Vec<String> = entries
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.name)
                    .collect();
                names.sort();
                names
            })
            .unwrap_or_default(),
        latest_run: None,
        links: None,
        readme: node.readme.map(|blob| match blob {
            None => crate::Readme {
                present: false,
                bytes: 0,
                badges: Vec::new(),
            },
            Some(blob) => {
                let text = blob.text.unwrap_or_default();
                crate::Readme {
                    present: true,
                    bytes: blob.byte_size.max(text.len() as u32),
                    badges: crate::badge::extract(&text),
                }
            }
        }),
        tracker: tracker_from(node.agents, node.claude),
        exempt: BTreeMap::new(),
    }
}

/// The tracker evidence from the instruction files a page carried.
///
/// `None` means the page never asked for them — an older collector, whose
/// answer is `unknown` rather than a repository wrongly called clean.
fn tracker_from(
    agents: Option<Option<Blob>>,
    claude: Option<Option<Blob>>,
) -> Option<crate::Tracker> {
    if agents.is_none() && claude.is_none() {
        return None;
    }
    let mut tracker = crate::Tracker::default();
    for (path, blob) in [("AGENTS.md", agents), ("CLAUDE.md", claude)] {
        let Some(Some(blob)) = blob else { continue };
        match blob.text {
            Some(text) => {
                tracker.files.push(path.to_string());
                tracker.stale.extend(crate::tracker::scan(path, &text));
            }
            // The forge returns a blob without `text` when it will not inline
            // it, which is the one case where a missing word is not evidence.
            None => tracker.unread.push(path.to_string()),
        }
    }
    Some(tracker)
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

#[derive(Debug, Deserialize)]
struct GraphQlDocument {
    #[serde(default)]
    data: Option<GraphQlData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlData {
    #[serde(default)]
    repository_owner: Option<Owner>,
    /// The single-repository shape, which is what a pull-request gate asks for.
    #[serde(default)]
    repository: Option<Node>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Owner {
    #[serde(default)]
    login: Option<String>,
    #[serde(default)]
    repositories: Option<Connection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    #[serde(default)]
    nodes: Vec<Option<Node>>,
}

// Not `deny_unknown_fields`: the forge grows fields, and a collector that
// refuses to read a page because GitHub added one is a collector that stops
// working on somebody else's release day.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    owner: Option<OwnerRef>,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    is_fork: bool,
    #[serde(default)]
    is_private: bool,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    license_info: Option<LicenseInfo>,
    #[serde(default)]
    default_branch_ref: Option<BranchRef>,
    #[serde(default)]
    releases: Option<Count>,
    #[serde(default)]
    pull_requests: Option<Count>,
    #[serde(default)]
    #[serde(rename = "branches")]
    branches: Option<Count>,
    #[serde(default)]
    workflows: Option<Tree>,
    // `Option<Option<_>>` on purpose, through `double_option`: a field the page
    // never asked for is `None`, and a field it asked for and got `null` back is
    // `Some(None)` — a README that is definitely absent. Collapsing the two
    // would turn "not collected" into "missing" and fail a repository nobody
    // read.
    #[serde(default, deserialize_with = "double_option")]
    readme: Option<Option<Blob>>,
    // The same double `Option` for the same reason: a page that did not ask for
    // AGENTS.md and a page that asked and got null are different facts, and the
    // tracker check reads them differently.
    #[serde(default, deserialize_with = "double_option")]
    agents: Option<Option<Blob>>,
    #[serde(default, deserialize_with = "double_option")]
    claude: Option<Option<Blob>>,
}

fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// A file at a revision. `None` for the whole field means the file is not
/// there, which is why the README answer can be a `fail` rather than an
/// `unknown`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Blob {
    #[serde(default)]
    byte_size: u32,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnerRef {
    #[serde(default)]
    login: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LicenseInfo {
    #[serde(default)]
    spdx_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchRef {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Count {
    #[serde(default)]
    total_count: u32,
}

#[derive(Debug, Deserialize)]
struct Tree {
    #[serde(default)]
    entries: Option<Vec<Option<Entry>>>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(default)]
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::Collector;

    const PAGE: &str = r#"
    {"data":{"repositoryOwner":{"login":"fpl","repositories":{"nodes":[
      {"name":"bob","isArchived":false,"isFork":false,"isPrivate":true,
       "pushedAt":"2026-11-17T10:54:14Z","description":"a build plane",
       "licenseInfo":null,"defaultBranchRef":{"name":"main"},
       "releases":{"totalCount":3},"pullRequests":{"totalCount":2},
       "branches":{"totalCount":28},
       "workflows":{"entries":[{"name":"release.yml"},{"name":"ci.yml"}]}},
      {"name":"fab","isArchived":false,"isFork":false,"isPrivate":true,
       "pushedAt":"2026-11-17T09:38:28Z","description":null,
       "licenseInfo":{"spdxId":"MIT"},"defaultBranchRef":{"name":"master"},
       "releases":{"totalCount":0},"pullRequests":{"totalCount":1},
       "branches":{"totalCount":71},"workflows":null}
    ]}}}}
    "#;

    #[test]
    fn maps_a_graphql_page_to_repository_state() {
        let mut collector = Collector::new();
        assert_eq!(collector.absorb_stream(PAGE.as_bytes()).unwrap(), 2);
        let snapshot = collector.finish(Some("2026-11-17T12:00:00Z".into()));

        assert_eq!(snapshot.repos.len(), 2);
        assert_eq!(snapshot.repos[0].slug(), "fpl/bob");
        assert_eq!(snapshot.repos[0].license, None);
        assert_eq!(snapshot.repos[0].workflows, vec!["ci.yml", "release.yml"]);
        assert_eq!(snapshot.repos[0].branches, 28);
        assert_eq!(snapshot.repos[0].default_branch.as_deref(), Some("main"));

        assert_eq!(snapshot.repos[1].license.as_deref(), Some("MIT"));
        assert!(snapshot.repos[1].workflows.is_empty());
        assert_eq!(snapshot.repos[1].description, None);
    }

    #[test]
    fn several_pages_and_several_owners_concatenate() {
        let page = r#"{"data":{"repositoryOwner":{"login":"other","repositories":{"nodes":[{"name":"thing"}]}}}}"#;
        let mut collector = Collector::new();
        collector.absorb_stream(PAGE.as_bytes()).unwrap();
        collector.absorb_stream(page.as_bytes()).unwrap();
        let snapshot = collector.finish(None);
        assert_eq!(snapshot.repos.len(), 3);
        assert_eq!(
            snapshot.orgs(),
            vec!["fpl".to_string(), "other".to_string()]
        );
    }

    #[test]
    fn a_page_with_only_errors_contributes_nothing_rather_than_failing() {
        let mut collector = Collector::new();
        let errors = r#"{"data":{"repositoryOwner":null},"errors":[{"message":"nope"}]}"#;
        assert_eq!(collector.absorb_stream(errors.as_bytes()).unwrap(), 0);
        assert!(collector.is_empty());
    }

    #[test]
    fn a_repository_seen_twice_is_stored_once() {
        let mut collector = Collector::new();
        collector.absorb_stream(PAGE.as_bytes()).unwrap();
        collector.absorb_stream(PAGE.as_bytes()).unwrap();
        assert_eq!(collector.len(), 2);
    }

    #[test]
    fn instruction_files_are_read_for_the_tracker_words_they_use() {
        let page = r#"{"data":{"repositoryOwner":{"login":"fpl","repositories":{"nodes":[
          {"name":"stale","agents":{"text":"Run `bd prime` before you start."},
           "claude":{"text":"work lives in `.beads/dolt`"}},
          {"name":"clean","agents":{"text":"Track work with `mb` in marbles."}},
          {"name":"nothing"}
        ]}}}}"#;
        let mut collector = Collector::new();
        collector.absorb_stream(page.as_bytes()).unwrap();
        let snapshot = collector.finish(None);

        // `finish` sorts by slug, so find the nodes by the name the page gave.
        let by_name = |name: &str| {
            snapshot
                .repos
                .iter()
                .find(|repo| repo.name == name)
                .expect("the page named this repository")
        };

        let tracker = by_name("stale").tracker.as_ref().expect("the page asked");
        assert_eq!(tracker.files, vec!["AGENTS.md", "CLAUDE.md"]);
        let words: Vec<&str> = tracker.stale.iter().map(|s| s.token.as_str()).collect();
        assert_eq!(words, vec!["bd", "beads"], "vocabulary order, one per word");
        assert!(tracker.unread.is_empty());

        let clean = by_name("clean").tracker.as_ref().expect("the page asked");
        assert!(clean.stale.is_empty());
        assert_eq!(clean.files, vec!["AGENTS.md"]);

        // A page that never asked is not a page that found nothing.
        assert!(by_name("nothing").tracker.is_none());
    }

    #[test]
    fn a_file_the_forge_will_not_inline_is_unread_rather_than_clean() {
        let page = r#"{"data":{"repository":{"owner":{"login":"fpl"},"name":"bob",
          "agents":{"byteSize":9000000,"text":null}}}}"#;
        let mut collector = Collector::new();
        collector.absorb_stream(page.as_bytes()).unwrap();
        let snapshot = collector.finish(None);
        let tracker = snapshot.repos[0].tracker.as_ref().expect("the page asked");
        assert_eq!(tracker.unread, vec!["AGENTS.md"]);
        assert!(tracker.stale.is_empty());
    }

    #[test]
    fn a_single_repository_document_lands_in_the_same_snapshot() {
        // What `collect.sh --repo` sends: the shape a pull-request gate asks
        // for. It has to produce the same RepoState as the fleet query, or the
        // gate and the nightly run would disagree about one repository.
        let single = r#"{"data":{"repository":{"name":"bob","isArchived":false,
          "isFork":false,"isPrivate":true,"description":"a build plane",
          "licenseInfo":null,"defaultBranchRef":{"name":"main"},
          "releases":{"totalCount":3},"pullRequests":{"totalCount":2},
          "branches":{"totalCount":28},"pushedAt":"2026-11-17T10:54:14Z",
          "workflows":{"entries":[{"name":"ci.yml"},{"name":"release.yml"}]},
          "owner":{"login":"fpl"}}}}"#;
        let mut collector = Collector::new();
        assert_eq!(collector.absorb_stream(single.as_bytes()).unwrap(), 1);
        let snapshot = collector.finish(None);
        assert_eq!(snapshot.repos.len(), 1);
        assert_eq!(snapshot.repos[0].slug(), "fpl/bob");
        assert_eq!(snapshot.orgs(), vec!["fpl".to_string()]);

        let mut fleet = Collector::new();
        fleet.absorb_stream(PAGE.as_bytes()).unwrap();
        let from_fleet = fleet.finish(None);
        let bob = from_fleet
            .repos
            .iter()
            .find(|repo| repo.name == "bob")
            .expect("bob is on the page");
        assert_eq!(&snapshot.repos[0], bob, "the two shapes agree");
    }

    #[test]
    fn a_single_repository_without_an_owner_is_dropped_rather_than_guessed() {
        let orphan = r#"{"data":{"repository":{"name":"bob"}}}"#;
        let mut collector = Collector::new();
        assert_eq!(collector.absorb_stream(orphan.as_bytes()).unwrap(), 0);
        assert!(collector.is_empty());
    }

    #[test]
    fn an_unreadable_document_is_an_error_not_a_silent_empty_fleet() {
        let mut collector = Collector::new();
        assert!(collector.absorb_stream(b"{not json}").is_err());
    }
}
