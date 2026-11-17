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

use arborist_wall::{Wall, report_from};
use gpui::{
    AnyWindowHandle, AppContext as _, HeadlessAppContext, PlatformTextSystem as _, prelude::*,
    px, size,
};
use gpui_wgpu::{CosmicTextSystem, WgpuContext, WgpuHeadlessRenderer};
use soil::present::surface;

const LILEX: &[u8] = include_bytes!("../assets/Lilex-Regular.ttf");
const LILEX_BOLD: &[u8] = include_bytes!("../assets/Lilex-Bold.ttf");
const SNAPSHOT: &str = include_str!("../../fixtures/fleet.json");

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
        |_, cx| cx.new(|_| Wall::new(report_from(SNAPSHOT))),
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
