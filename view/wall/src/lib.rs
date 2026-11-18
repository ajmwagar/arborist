//! The fleet wall: arborist's report, drawn by GPUI.
//!
//! This is the view and only the view. It takes the `Report` the command prints
//! — same type, same values — and lays it out. There is no evaluator here, which
//! is the point: a wall that computed its own numbers would eventually disagree
//! with the gate, and the wall is the thing people believe.
//!
//! Two hosts mount it and neither is named here: a `wasm32-wasip2` Spore that
//! renders offscreen and presents a texture, and a browser that renders into a
//! canvas through `gpui_web`. The same file, the same pixels.

use arborist::check::Severity;
use arborist::report::CheckSummary;
use arborist::Report;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Render, SharedString, Window, div, prelude::*,
    px, rgb,
};

/// A report from a snapshot document. Both hosts start here; where the bytes
/// came from is theirs to know.
///
/// # Panics
///
/// Panics if `json` is not a snapshot. A host ships its own document, so an
/// unreadable one is a build mistake rather than a runtime condition.
#[must_use]
pub fn report_from(json: &str) -> Report {
    let snapshot: arborist::Snapshot =
        serde_json::from_str(json).expect("the embedded snapshot is a snapshot");
    arborist::evaluate(&snapshot)
}

/// The report `arborist report --json` wrote, as the wall draws it.
///
/// The view renders the same [`Report`] the gate reads, so a page that fetched
/// the file needs no evaluator and no snapshot: the numbers on the wall are the
/// numbers the command printed, character for character.
///
/// `None` when the text is not a report — a 404 page, a proxy error, a half
/// written file. The caller falls back to the compiled-in snapshot rather than
/// drawing a wall of zeroes that looks like a healthy fleet.
#[must_use]
pub fn report_from_document(json: &str) -> Option<Report> {
    serde_json::from_str(json).ok()
}

const HEADER_H: f32 = 64.0;
const GAP: f32 = 18.0;
/// Repositories named on the wall. The report carries all of them; a wall that
/// scrolls is a wall that is read once.
const ROWS: usize = 12;
/// Checks shown per repository before the rest become a count.
const CHIPS: usize = 4;

const BG: u32 = 0x0b0f14;
const PANEL: u32 = 0x111820;
const BORDER: u32 = 0x1c2733;
const TEXT: u32 = 0xe6edf5;
const MUTED: u32 = 0x76828f;
const ACCENT: u32 = 0xa3be8c;
const CRITICAL: u32 = 0xd9808b;
const ADVISORY: u32 = 0xd7b06a;
const UNKNOWN: u32 = 0x5f6b78;

fn severity_colour(severity: Severity) -> u32 {
    match severity {
        Severity::Critical => CRITICAL,
        Severity::Advisory => ADVISORY,
    }
}

/// The whole screen: one report, read once.
///
/// The wall owns the layout and nothing else. Where the report came from — a
/// fixture compiled into a Spore, a fetch over the bridge, a file a host read —
/// is the host's decision, and this type never learns which.
pub struct Wall {
    report: Report,
}

impl Wall {
    #[must_use]
    pub fn new(report: Report) -> Self {
        Self { report }
    }

    fn header(&self) -> AnyElement {
        let owners = if self.report.orgs.is_empty() {
            "no owner".to_string()
        } else {
            self.report.orgs.join(" · ")
        };
        let collected = self
            .report
            .collected_at
            .clone()
            .unwrap_or_else(|| "uncollected".to_string());
        div()
            .h(px(HEADER_H))
            .flex()
            .items_center()
            .justify_between()
            .px(px(28.0))
            .bg(rgb(PANEL))
            .border_b(px(1.0))
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .child(
                        div()
                            .text_size(px(19.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(ACCENT))
                            .child(SharedString::from("arborist")),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child(SharedString::from(format!("{owners} · {collected}"))),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(SharedString::from(format!(
                        "{} repositories · {} skipped",
                        self.report.summary.checked, self.report.summary.skipped
                    ))),
            )
            .into_any_element()
    }

    /// The one sentence the tool exists to produce, above the numbers that
    /// support it. A wall whose first line is a count of repositories is a wall
    /// nobody reads twice.
    fn headline(&self) -> AnyElement {
        let colour = if self.report.summary.critical > 0 {
            CRITICAL
        } else if self.report.summary.advisory > 0 {
            ADVISORY
        } else {
            ACCENT
        };
        div()
            .px(px(28.0))
            .pt(px(18.0))
            .child(
                div()
                    .text_size(px(15.0))
                    .text_color(rgb(colour))
                    .child(SharedString::from(
                        arborist::digest::digest(&self.report).headline,
                    )),
            )
            .into_any_element()
    }

    /// Four numbers, because those are the four answers: broken, drifting,
    /// clean, and unmeasured.
    fn stats(&self) -> AnyElement {
        let summary = &self.report.summary;
        div()
            .flex()
            .gap(px(GAP))
            .child(stat("failing critical", summary.critical, CRITICAL))
            .child(stat("advisory only", summary.advisory, ADVISORY))
            .child(stat("clean", summary.clean, ACCENT))
            .child(stat("with an unknown", summary.unknown, UNKNOWN))
            .into_any_element()
    }

    fn checks(&self) -> AnyElement {
        let checked = self.report.summary.checked.max(1);
        let mut table = div().flex().flex_col().gap(px(9.0));
        for entry in &self.report.summary.by_check {
            table = table.child(check_row(entry, checked));
        }
        panel("checks", "what the fleet is asked, and how many said no")
            .child(table)
            .into_any_element()
    }

    fn worst(&self) -> AnyElement {
        let worst = self.report.worst(false);
        let mut list = div().flex().flex_col().gap(px(10.0));
        if worst.is_empty() {
            list = list.child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child(SharedString::from("no repository failed a check")),
            );
        }
        for repo in worst.iter().take(ROWS) {
            let mut chips = div().flex().items_center().gap(px(6.0));
            let mut worst_severity = Severity::Advisory;
            let failures: Vec<_> = repo.fails().collect();
            for finding in &failures {
                if finding.severity > worst_severity {
                    worst_severity = finding.severity;
                }
            }
            for finding in failures.iter().take(CHIPS) {
                chips = chips.child(chip(finding.check.id(), finding.severity));
            }
            if failures.len() > CHIPS {
                chips = chips.child(more(failures.len() - CHIPS));
            }
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(18.0))
                                    .bg(rgb(severity_colour(worst_severity))),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(rgb(TEXT))
                                    .child(SharedString::from(repo.slug())),
                            ),
                    )
                    .child(chips),
            );
        }
        panel(
            "worst first",
            &format!("{} repositories with findings", worst.len()),
        )
        .child(list)
        .into_any_element()
    }
}

fn stat(label: &str, value: usize, colour: u32) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .p(px(16.0))
        .bg(rgb(PANEL))
        .border(px(1.0))
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(30.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(colour))
                .child(SharedString::from(value.to_string())),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(SharedString::from(label.to_owned())),
        )
        .into_any_element()
}

fn panel(title: &str, subtitle: &str) -> gpui::Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .p(px(16.0))
        .bg(rgb(PANEL))
        .border(px(1.0))
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child(SharedString::from(title.to_owned())),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(MUTED))
                        .child(SharedString::from(subtitle.to_owned())),
                ),
        )
}

fn check_row(entry: &CheckSummary, checked: usize) -> AnyElement {
    let share = entry.fail as f32 / checked as f32;
    let colour = severity_colour(entry.severity);
    let mut bar = div().w(px(180.0)).h(px(6.0)).bg(rgb(0x161f28));
    bar = bar.child(
        div()
            .w(px((180.0 * share).max(if entry.fail > 0 { 2.0 } else { 0.0 })))
            .h(px(6.0))
            .bg(rgb(colour)),
    );
    let unknown = if entry.unknown > 0 {
        format!("{} unmeasured", entry.unknown)
    } else {
        String::new()
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .w(px(92.0))
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
                .child(SharedString::from(entry.check.id())),
        )
        .child(bar)
        .child(
            div()
                .w(px(52.0))
                .text_size(px(12.0))
                .text_color(rgb(if entry.fail > 0 { colour } else { MUTED }))
                .child(SharedString::from(entry.fail.to_string())),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .child(SharedString::from(unknown)),
        )
        .into_any_element()
}

fn more(hidden: usize) -> AnyElement {
    div()
        .px(px(7.0))
        .py(px(2.0))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .child(SharedString::from(format!("+{hidden}"))),
        )
        .into_any_element()
}

fn chip(check: &str, severity: Severity) -> AnyElement {
    div()
        .px(px(7.0))
        .py(px(2.0))
        .bg(rgb(severity_colour(severity)))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(0x0b0f14))
                .child(SharedString::from(check.to_owned())),
        )
        .into_any_element()
}

impl Render for Wall {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .font_family("Lilex")
            .child(self.header())
            .child(self.headline())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .p(px(28.0))
                    .child(self.stats())
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .gap(px(GAP))
                            .child(self.checks())
                            .child(self.worst()),
                    ),
            )
    }
}
