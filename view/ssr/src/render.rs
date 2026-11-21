//! The pages. One report in, HTML out.
//!
//! Every number here comes from [`arborist::Report`]. The page does no
//! arithmetic the report has not already done, so the wall and the gate cannot
//! drift apart: there is nothing to keep in step.

use arborist::{Check, RepoReport, Report, Severity, Status};
use maud::{DOCTYPE, Markup, html};

/// Where the report came from. The page says it out loud, because a fixture
/// that looks like a live fleet is the calm dashboard this tool exists to
/// prevent.
pub enum Source {
    Snapshot(String),
    Report(String),
    Fixture,
}

impl Source {
    fn label(&self) -> Markup {
        match self {
            Self::Snapshot(path) => html! { "snapshot " code { (path) } },
            Self::Report(path) => html! { "report " code { (path) } },
            Self::Fixture => html! {
                span.fixture { "fixture" }
                " compiled in — mount "
                code { "snapshot.json" }
                " to score a real fleet"
            },
        }
    }
}

const STYLE: &str = r"
:root {
  --bg: #0b0f14; --panel: #111820; --border: #1c2733; --text: #e6edf5;
  --muted: #76828f; --accent: #a3be8c; --critical: #d9808b;
  --advisory: #d7b06a; --unknown: #5f6b78;
}
* { box-sizing: border-box; }
html, body { margin: 0; background: var(--bg); color: var(--text); }
body {
  font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
  padding: 32px 28px 64px;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
header { display: flex; align-items: baseline; gap: 14px; flex-wrap: wrap; }
h1 { font-size: 22px; margin: 0; letter-spacing: .02em; }
h2 { font-size: 13px; font-weight: 600; color: var(--muted);
     text-transform: uppercase; letter-spacing: .08em; margin: 36px 0 12px; }
.sub { color: var(--muted); }
.source { margin-left: auto; color: var(--muted); font-size: 11px; }
.source code { color: var(--text); }
.fixture { color: var(--advisory); border: 1px solid var(--advisory);
           border-radius: 3px; padding: 0 5px; }
.headline { margin: 22px 0 0; max-width: 82ch; font-size: 15px; }
.tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
         gap: 10px; margin-top: 20px; }
.tile { background: var(--panel); border: 1px solid var(--border);
        border-radius: 6px; padding: 12px 14px; }
.tile .n { font-size: 26px; line-height: 1.1; }
.tile .k { color: var(--muted); font-size: 11px; text-transform: uppercase;
           letter-spacing: .06em; margin-top: 4px; }
.tile.critical .n { color: var(--critical); }
.tile.advisory .n { color: var(--advisory); }
.tile.clean .n { color: var(--accent); }
.tile.unknown .n { color: var(--unknown); }
.bars { display: grid; gap: 6px; }
.bar { display: grid; grid-template-columns: 150px 1fr 92px; gap: 12px;
       align-items: center; }
.bar .name { color: var(--text); }
.bar .name .sev { color: var(--muted); font-size: 10px; margin-left: 6px; }
.track { height: 8px; background: var(--panel); border: 1px solid var(--border);
         border-radius: 999px; overflow: hidden; }
.fill { height: 100%; background: var(--critical); }
.fill.advisory { background: var(--advisory); }
.fill.none { background: var(--unknown); }
.bar .count { color: var(--muted); text-align: right; font-size: 12px; }
.bar .count b { color: var(--text); font-weight: 600; }
.repos { display: grid; gap: 8px; }
.repo { background: var(--panel); border: 1px solid var(--border);
        border-radius: 6px; padding: 10px 12px; display: grid;
        grid-template-columns: 1fr auto; gap: 6px 14px; }
.repo .slug { font-size: 13px; }
.repo .chips { grid-column: 1 / -1; display: flex; flex-wrap: wrap; gap: 5px; }
.chip { font-size: 11px; border: 1px solid var(--border); border-radius: 3px;
        padding: 1px 6px; color: var(--muted); }
.chip.critical { border-color: var(--critical); color: var(--critical); }
.chip.advisory { border-color: var(--advisory); color: var(--advisory); }
.badge { font-size: 10px; text-transform: uppercase; letter-spacing: .08em;
         border-radius: 3px; padding: 2px 7px; align-self: start; }
.badge.critical { color: var(--critical); border: 1px solid var(--critical); }
.badge.advisory { color: var(--advisory); border: 1px solid var(--advisory); }
.badge.unknown { color: var(--unknown); border: 1px solid var(--unknown); }
.badge.clean { color: var(--accent); border: 1px solid var(--accent); }
table { width: 100%; border-collapse: collapse; }
th, td { text-align: left; padding: 7px 10px; border-bottom: 1px solid var(--border);
         vertical-align: top; }
th { color: var(--muted); font-weight: 500; font-size: 11px;
     text-transform: uppercase; letter-spacing: .06em; }
td.num { text-align: right; font-variant-numeric: tabular-nums; }
td.fail { color: var(--critical); }
td.unknown { color: var(--unknown); }
tr:last-child td { border-bottom: none; }
.findings { display: grid; gap: 6px; }
.finding { background: var(--panel); border: 1px solid var(--border);
           border-radius: 6px; padding: 9px 12px; }
.finding .top { display: flex; gap: 10px; align-items: baseline; }
.finding .check { font-weight: 600; }
.finding .detail { color: var(--muted); margin-top: 3px; }
.finding.fail .check { color: var(--critical); }
.finding.unknown .check { color: var(--unknown); }
.finding.pass .check { color: var(--accent); }
.finding.exempt .check { color: var(--muted); }
.crumbs { margin-bottom: 18px; color: var(--muted); }
.empty { color: var(--muted); }
footer { margin-top: 40px; color: var(--muted); font-size: 11px; }
";

fn layout(title: &str, source: &Source, body: Markup) -> String {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width,initial-scale=1";
                title { (title) }
                style { (STYLE) }
            }
            body {
                header {
                    h1 { "arborist" }
                    span.sub { "fleet source health" }
                    span.source { (source.label()) }
                }
                (body)
                footer {
                    "The same " code { "arborist::Report" }
                    " the command prints. "
                    a href="/checks" { "every check" }
                    " · "
                    a href="/report.json" { "report.json" }
                }
            }
        }
    }
    .into_string()
}

pub fn dashboard(report: &Report, source: &Source) -> String {
    let s = &report.summary;
    let failing = report.failing_count();
    let headline = if s.checked == 0 {
        "This snapshot scored no repositories.".to_string()
    } else {
        format!(
            "{} repositories scored: {} failing a critical check, {} advisory only, {} clean, {} unmeasured.",
            s.checked, s.critical, s.advisory, s.clean, s.unknown
        )
    };

    let worst: Vec<&RepoReport> = report.worst(true).into_iter().take(24).collect();

    layout(
        "arborist",
        source,
        html! {
            p.headline { (headline) }
            div.tiles {
                div.tile { div.n { (s.checked) } div.k { "scored" } }
                div.tile.critical { div.n { (s.critical) } div.k { "critical" } }
                div.tile.advisory { div.n { (s.advisory) } div.k { "advisory only" } }
                div.tile.clean { div.n { (s.clean) } div.k { "clean" } }
                div.tile.unknown { div.n { (s.unknown) } div.k { "unmeasured" } }
                div.tile { div.n { (s.skipped) } div.k { "skipped" } }
            }

            h2 { "the shape of the distribution" }
            div.bars {
                @for entry in &s.by_check {
                    @let share = if s.checked == 0 { 0.0 }
                        else { entry.fail as f64 / s.checked as f64 };
                    @let fill = if entry.fail == 0 { "fill none" }
                        else if entry.severity == Severity::Advisory { "fill advisory" }
                        else { "fill" };
                    div.bar {
                        div.name {
                            a href="/checks" { (entry.check.id()) }
                            span.sev { (entry.severity.id()) }
                        }
                        div.track {
                            div class=(fill) style=(format!("width:{:.1}%", (share * 100.0).max(if entry.fail == 0 { 0.6 } else { 1.2 }))) {}
                        }
                        div.count {
                            b { (entry.fail) } " fail"
                            @if entry.unknown > 0 { " · " (entry.unknown) " ?" }
                        }
                    }
                }
            }

            h2 {
                "worth opening first"
                @if failing > worst.len() {
                    span.sub { " — " (worst.len()) " of " (failing) }
                }
            }
            @if worst.is_empty() {
                p.empty { "No repository failed a check. That is either good news or an empty snapshot." }
            } @else {
                div.repos {
                    @for repo in worst {
                        (repo_card(repo))
                    }
                }
            }
        },
    )
}

fn repo_card(repo: &RepoReport) -> Markup {
    let fails: Vec<_> = repo.fails().collect();
    let badge = match repo.worst_fail() {
        Some(Severity::Critical) => ("critical", "critical"),
        Some(Severity::Advisory) => ("advisory", "advisory"),
        None => ("unknown", "unmeasured"),
    };
    let href = format!(
        "/repo?org={}&name={}",
        encode(&repo.org),
        encode(&repo.name)
    );
    html! {
        div.repo {
            div.slug { a href=(href) { (repo.slug()) } }
            span class={ "badge " (badge.0) } { (badge.1) }
            div.chips {
                @for finding in fails.iter().take(6) {
                    span class={ "chip " (finding.severity.id()) } { (finding.check.id()) }
                }
                @if fails.len() > 6 {
                    span.chip { "+" (fails.len() - 6) " more" }
                }
                @if fails.is_empty() {
                    span.chip { "no failing checks" }
                }
            }
        }
    }
}

pub fn checks_page(report: &Report, source: &Source) -> String {
    let checked = report.summary.checked;
    layout(
        "arborist · checks",
        source,
        html! {
            div.crumbs { a href="/" { "arborist" } " / checks" }
            h2 { "every question, and what to do about it" }
            table {
                thead {
                    tr {
                        th { "check" }
                        th { "severity" }
                        th.num { "fail" }
                        th.num { "unknown" }
                        th.num { "pass" }
                        th.num { "exempt" }
                        th { "remedy" }
                    }
                }
                tbody {
                    @for entry in &report.summary.by_check {
                        @let fail_class = if entry.fail > 0 { "num fail" } else { "num" };
                        @let unknown_class = if entry.unknown > 0 { "num unknown" } else { "num" };
                        tr {
                            td { a href=(format!("#{}", entry.check.id())) { (entry.check.id()) } }
                            td { (entry.severity.id()) }
                            td class=(fail_class) { (entry.fail) }
                            td class=(unknown_class) { (entry.unknown) }
                            td.num { (entry.pass) }
                            td.num { (entry.exempt) }
                            td { (entry.check.remedy()) }
                        }
                    }
                }
            }
            h2 { "the questions" }
            table {
                thead { tr { th { "check" } th { "question" } } }
                tbody {
                    @for check in Check::ALL {
                        tr {
                            td id=(check.id()) { (check.id()) }
                            td { (check.question()) }
                        }
                    }
                }
            }
            p.empty {
                (checked) " repositories in this report. "
                "Unknown is not a pass: a check with no evidence is counted separately."
            }
        },
    )
}

pub fn repo_page(report: &Report, source: &Source, org: &str, name: &str) -> Option<String> {
    let repo = report
        .repos
        .iter()
        .find(|repo| repo.org == org && repo.name == name)?;
    let badge = match repo.worst_fail() {
        Some(Severity::Critical) => ("critical", "critical"),
        Some(Severity::Advisory) => ("advisory", "advisory"),
        None if repo.has_unknown() => ("unknown", "unmeasured"),
        None => ("clean", "clean"),
    };
    Some(layout(
        &format!("arborist · {}", repo.slug()),
        source,
        html! {
            div.crumbs { a href="/" { "arborist" } " / " (repo.slug()) }
            h2 { (repo.slug()) span class={ "badge " (badge.0) } { (badge.1) } }
            div.findings {
                @for finding in &repo.findings {
                    @let (class, label, detail) = match &finding.status {
                        Status::Pass => ("pass", "pass", String::new()),
                        Status::Fail { detail } => ("fail", "fail", detail.clone()),
                        Status::Unknown { detail } => ("unknown", "unknown", detail.clone()),
                        Status::Exempt { reason } => ("exempt", "exempt", reason.clone()),
                    };
                    div class={ "finding " (class) } {
                        div.top {
                            span.check { (finding.check.id()) }
                            span.sub { (finding.severity.id()) " · " (label) }
                        }
                        @if !detail.is_empty() {
                            div.detail { (detail) }
                        }
                    }
                }
            }
            @if repo.worst_fail().is_none() && !repo.has_unknown() {
                p.empty { "Nothing failed and nothing was left unmeasured." }
            }
        },
    ))
}

pub fn error_page(message: &str) -> String {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { "arborist" }
                style { (STYLE) }
            }
            body {
                header { h1 { "arborist" } span.sub { "fleet source health" } }
                p.headline { "This page could not be rendered." }
                p.empty { (message) }
                footer { a href="/" { "back to the report" } }
            }
        }
    }
    .into_string()
}

/// Percent-encode a path segment. Owner and repository names are nearly boring
/// already; encoding them keeps a query string from becoming an injection.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            byte => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
