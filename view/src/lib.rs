//! The fleet wall: arborist's report, drawn by GPUI.
//!
//! A Spore owns its view. This one owns nothing else: it takes the `Report` the
//! command prints — same type, same values — and lays it out. There is no second
//! evaluator here, which is the point. A wall that computed its own numbers
//! would eventually disagree with the gate, and the wall is the thing people
//! believe.
//!
//! ## Where the report comes from
//!
//! `fixtures/fleet.json` is compiled in: a snapshot with the real fleet's shape
//! and invented names, so the frame surface can render one without a host that
//! serves anything. Fetching it over `soil:http` is the next step, and it is a
//! change to where `SNAPSHOT` is read, not to the view.

wit_bindgen::generate!({
    path: "wit",
    world: "runner",
    with: {
        "wasi:webgpu/webgpu@0.3.0-rc.2": wgpu_wasi_webgpu::wasi::webgpu::webgpu,
    },
    generate_all,
});

use std::{borrow::Cow, sync::Arc};

use arborist::check::Severity;
use arborist::report::CheckSummary;
use arborist::{Report, Snapshot, evaluate};
use gpui::{
    AnyElement, AnyWindowHandle, AppContext as _, Context, FontWeight, HeadlessAppContext,
    IntoElement, PlatformTextSystem as _, Render, SharedString, Window, div, prelude::*, px, rgb,
    size,
};
use gpui_wgpu::{CosmicTextSystem, WgpuContext, WgpuHeadlessRenderer};
use soil::present::{surface};

const LILEX: &[u8] = include_bytes!("../assets/Lilex-Regular.ttf");
const LILEX_BOLD: &[u8] = include_bytes!("../assets/Lilex-Bold.ttf");
const SNAPSHOT: &str = include_str!("../../fixtures/fleet.json");

const HEADER_H: f32 = 64.0;
const GAP: f32 = 18.0;

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
struct Wall {
    report: Report,
}

impl Wall {
    fn new() -> Self {
        let snapshot: Snapshot =
            serde_json::from_str(SNAPSHOT).expect("the fixture is a snapshot");
        Self {
            report: evaluate(&snapshot),
        }
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
            .bg(PANEL)
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
        for repo in worst.iter().take(8) {
            let mut chips = div().flex().items_center().gap(px(6.0));
            let mut worst_severity = Severity::Advisory;
            for finding in repo.fails() {
                if finding.severity > worst_severity {
                    worst_severity = finding.severity;
                }
                chips = chips.child(chip(finding.check.id(), finding.severity));
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
        .bg(PANEL)
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
        .bg(PANEL)
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
    let mut bar = div().w(px(180.0)).h(px(6.0)).bg(0x161f28u32 as u32);
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
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .p(px(28.0))
                    .child(self.stats())
                    .child(
                        div()
                            .flex()
                            .gap(px(GAP))
                            .child(self.checks())
                            .child(self.worst()),
                    ),
            )
    }
}

fn present_frame(
    context: &WgpuContext,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> anyhow::Result<()> {
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("arborist frame"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    context.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let texture = wgpu_wasi_webgpu::host_texture(texture);
    surface::present(&texture).map_err(|error| anyhow::anyhow!("present: {error}"))?;
    Ok(())
}

fn run_wall() -> anyhow::Result<()> {
    let (width, height) = surface::size();
    let instance = wgpu_wasi_webgpu::instance();
    let adapter = wit_bindgen::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) = wit_bindgen::block_on(adapter.request_device(&Default::default()))?;
    let (context, format) = WgpuContext::from_offscreen(instance, adapter, device, queue)?;

    // A Spore carries its own fonts: the guest has no system to ask, and shaping
    // with no face traps in the text system.
    let text = Arc::new(CosmicTextSystem::new_without_system_fonts("Lilex"));
    text.add_fonts(vec![Cow::Borrowed(LILEX), Cow::Borrowed(LILEX_BOLD)])?;
    let factory_context = context.clone();
    let mut cx = HeadlessAppContext::with_platform(text, Arc::new(()), move || {
        Ok(Some(Box::new(WgpuHeadlessRenderer::with_context(
            factory_context.clone(),
            format,
        )?)))
    });

    let scale = surface::scale() as f32;
    let window = cx.open_window_scaled(
        size(px(width as f32), px(height as f32)),
        scale,
        |_, cx| cx.new(|_| Wall::new()),
    )?;
    let any: AnyWindowHandle = window.into();

    cx.update_window(any, |_, window, app| {
        let _ = window.draw(app);
    })?;
    let image = cx.capture_screenshot(any)?;
    present_frame(&context, image.width(), image.height(), image.as_raw())?;
    Ok(())
}

struct Component;

impl Guest for Component {
    async fn run() -> u32 {
        match run_wall() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("arborist-view: {error:#}");
                1
            }
        }
    }
}

export!(Component);
