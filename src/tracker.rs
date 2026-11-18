//! The tracker a repository's agent instructions name, and the one they should.
//!
//! The fleet moved from beads — `bd`, a Dolt database per checkout — to marbles,
//! `mb` against one records book. An instruction file that still says
//! `bd prime` is not merely stale: it is an agent writing work into a place the
//! rest of the fleet never looks, which is how a fixed repository silently
//! becomes an unfixed one.
//!
//! The scan keeps the word, not the prose. "AGENTS.md says beads" is every fact
//! a report and a task body need, and it is four bytes instead of four
//! kilobytes — the same reason [`crate::Readme`] keeps badge URLs and not text.

use serde::{Deserialize, Serialize};

/// What the agent instruction files say, once somebody has read them.
///
/// `None` on [`crate::RepoState`] means nobody looked, which is a different fact
/// from a repository whose instructions were read and are clean.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tracker {
    /// The instruction files that exist on the default branch, by path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// One entry per stale word, naming the file that used it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale: Vec<Stale>,
    /// Files the forge would not hand over as text. A file nobody could read is
    /// not a clean file, so the check answers `unknown` rather than `pass`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unread: Vec<String>,
}

/// One instruction file still naming the tracker this replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stale {
    /// The path the word was found in, as the remedy prints it.
    pub path: String,
    /// The word itself, lowercased.
    pub token: String,
}

/// The tracker the fleet uses now, as the remedy names it.
pub const CURRENT: &str = "marbles (`mb`)";

/// Words that mean a file still points at the tracker marbles replaced.
///
/// Matched whole and case-insensitively, so `beads`, `.beads/` and `bd prime`
/// match while `bands`, `abdicate` and `subdural` do not. The list is short on
/// purpose: a table of every spelling of a dead tool is a table nobody keeps
/// current, and two words find every instruction file the fleet has.
pub const STALE: [&str; 2] = ["beads", "bd"];

/// The stale words `text` uses, one entry each, in [`STALE`] order.
///
/// One entry per word and file, not per occurrence: a file that says `bd` nine
/// times is one thing to fix, and nine identical lines in a task body is noise
/// that makes the one-line fix look bigger than it is.
#[must_use]
pub fn scan(path: &str, text: &str) -> Vec<Stale> {
    let folded = text.to_lowercase();
    let haystack = folded.as_bytes();
    let mut found = Vec::new();
    for token in STALE {
        if contains_word(haystack, token.as_bytes()) {
            found.push(Stale {
                path: path.to_string(),
                token: token.to_string(),
            });
        }
    }
    found
}

/// Whether `needle` appears in `haystack` between non-word characters.
fn contains_word(haystack: &[u8], needle: &[u8]) -> bool {
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
                    .is_some_and(|byte| is_word(*byte))
                && !haystack
                    .get(index + needle.len())
                    .is_some_and(|byte| is_word(*byte))
        })
}

/// A byte that continues a word. `_` counts, because `bd_prime` is one token to
/// a shell and to a reader.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::{STALE, scan};

    fn tokens(path: &str, text: &str) -> Vec<String> {
        scan(path, text)
            .into_iter()
            .map(|stale| stale.token)
            .collect()
    }

    #[test]
    fn a_command_in_prose_is_found() {
        assert_eq!(
            tokens("AGENTS.md", "Run `bd prime` before you start.\n"),
            vec!["bd"]
        );
    }

    #[test]
    fn the_long_name_is_found_wherever_it_sits() {
        assert_eq!(
            tokens("CLAUDE.md", "Issues live in `.beads/dolt/`."),
            vec!["beads"]
        );
        assert_eq!(tokens("CLAUDE.md", "BEADS integration"), vec!["beads"]);
    }

    #[test]
    fn words_that_merely_contain_the_token_are_not_found() {
        // The reason this is a word match and not a substring: these three are
        // how a check that cries wolf teaches a reader to ignore it.
        assert!(tokens("AGENTS.md", "bands, abdicate, subdural").is_empty());
    }

    #[test]
    fn a_file_that_says_it_twice_is_one_thing_to_fix() {
        assert_eq!(
            tokens("AGENTS.md", "bd ready\nbd close\nbd prime"),
            vec!["bd"]
        );
    }

    #[test]
    fn both_words_in_one_file_are_two_entries() {
        let found = scan("AGENTS.md", "migrated off beads; `bd` is gone");
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|stale| (stale.path.as_str(), stale.token.as_str()))
            .collect();
        assert_eq!(pairs, vec![("AGENTS.md", "beads"), ("AGENTS.md", "bd")]);
    }

    #[test]
    fn a_clean_file_says_nothing() {
        assert!(tokens("AGENTS.md", "Track work with `mb` in marbles.").is_empty());
    }

    #[test]
    fn the_vocabulary_is_lowercase_and_short() {
        for token in STALE {
            assert_eq!(token, token.to_lowercase(), "{token} must be lowercase");
        }
    }
}
