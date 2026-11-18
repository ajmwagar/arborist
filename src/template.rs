//! The fixes, so a finding arrives with something to do.
//!
//! A report that says "no CI" and stops has moved the work to the reader. These
//! are the files a reader should start from, compiled into the component so a
//! shell with no filesystem can still print one, and so the template a remedy
//! names is the template that shipped and not a copy that drifted.
//!
//! The templates are deliberately small. A template that does everything is one
//! nobody reads before deleting half of it; these carry the shape and the one
//! decision that matters in each place.

/// The README a repository should have: what it is, how to run it, and a status
/// badge that points at a workflow that exists.
pub const README: &str = include_str!("../templates/README.md");

/// The workflow that badge points at.
pub const CI: &str = include_str!("../templates/ci.yml");

/// The block that tells an agent which tracker to use, replacing the beads one
/// the fleet left behind.
pub const TRACKER: &str = include_str!("../templates/AGENTS.md");

/// The templates this build carries.
pub const NAMES: [&str; 3] = ["readme", "ci", "tracker"];

/// One template's text, or `None` for a name this build does not carry.
#[must_use]
pub fn get(name: &str) -> Option<&'static str> {
    match name {
        "readme" | "README" | "README.md" => Some(README),
        "ci" | "ci.yml" => Some(CI),
        "tracker" | "agents" | "AGENTS.md" => Some(TRACKER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{CI, NAMES, README, TRACKER, get};

    #[test]
    fn every_named_template_is_present_and_not_empty() {
        for name in NAMES {
            let text = get(name).unwrap_or_else(|| panic!("{name} is not carried"));
            assert!(text.len() > 100, "{name} is too small to be a template");
        }
    }

    #[test]
    fn an_unknown_name_is_none_rather_than_a_panic() {
        assert!(get("kotlin").is_none());
    }

    #[test]
    fn the_tracker_template_names_marbles_and_never_beads() {
        // The whole point of the template is that pasting it closes the check,
        // so the scan the check uses must come back empty on it.
        assert!(
            crate::tracker::scan("AGENTS.md", TRACKER).is_empty(),
            "{TRACKER}"
        );
        assert!(TRACKER.contains("`mb`"), "{TRACKER}");
    }

    #[test]
    fn the_readme_template_tells_the_reader_to_point_the_badge_at_the_workflow() {
        // The whole badges check is this one idea, and the template is where a
        // person first meets it.
        assert!(
            README.contains("actions/workflows/ci.yml/badge.svg"),
            "{README}"
        );
        assert!(CI.contains("name: ci"), "{CI}");
    }
}
