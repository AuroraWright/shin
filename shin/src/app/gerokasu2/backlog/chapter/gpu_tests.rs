//! Actual atlas/font host check. Root schedules this ignored hardware case.
use super::*;
use crate::{
    app::gerokasu2::tests::assets, asset::font::gerokasu2::draw::gpu_tests::Offscreen,
    render::PreRenderContext,
};
use shin_core::format::save::gerokasu2::{GameData, Progress, SaveDate};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};
struct Frames {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
}
impl Frames {
    fn new(gpu: &Offscreen) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Ger chapter panel readback"),
            size: wgpu::Extent3d {
                width: 320,
                height: 180,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Ger chapter stencil readback"),
                size: wgpu::Extent3d {
                    width: 320,
                    height: 180,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            view: texture.create_view(&Default::default()),
            texture,
            depth,
            resize: SurfaceResizeSource::new(ViewportParams::both((320, 180).into())),
            samplers: TextureSamplerStore::new(&gpu.device),
            pipelines: PipelineStorage::new(gpu.device.clone(), shin_render::TEXTURE_FORMAT),
            dynamic: DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(2 * 1024 * 1024)),
        }
    }
    fn draw(&mut self, gpu: &Offscreen, host: &ChapterHost) -> Vec<u8> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &gpu.device,
            queue: &gpu.queue,
            breakup_texture: None,
            resize_source: &self.resize,
            sampler_store: &self.samplers,
            depth_stencil: DepthStencilTarget { view: &self.depth },
            pipeline_storage: &mut self.pipelines,
            dynamic_buffer: &mut self.dynamic,
            encoder: &mut encoder,
        };
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &self.view,
                },
                Some(ctx.depth_stencil),
                "Ger ChapterSelector host",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            host.render(&mut pass).unwrap();
        }
        let pixels = crate::layer::user::readback_test_texture(&mut ctx, &self.texture);
        self.dynamic.recall();
        pixels
    }
}
fn history() -> History {
    History {
        entries: (0..16)
            .map(|i| backlog::history::Entry {
                boundary: i + 1,
                item: if i % 2 == 0 {
                    Item::Choice {
                        title: format!("日本語の章{i}、長い見出しを右揃えで表示"),
                        text: "選択した道".into(),
                        response: 0,
                    }
                } else {
                    Item::Checkpoint {
                        title: "記録した場面".into(),
                        text: format!("セーブ見出し{i}"),
                    }
                },
            })
            .collect(),
        chapters: (0..16).collect(),
        current_row: Some(7),
        jump_source: GameData {
            date: SaveDate::default(),
            progress: Progress::fresh(0, 42),
        },
    }
}
#[test]
#[ignore = "actual Ger chapter atlas/system font, gradients, clipped labels and cache reversal on Metal/exact Loopback Audio"]
fn ger_backlog_chapter_host_renders_native_panel_clip_labels_and_retains_cache_order() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let mut host = ChapterHost::load(assets, &history(), 5, 7).unwrap();
    host.update(5., input(), Vec2::splat(-1.), false, 0.016);
    host.update(
        1.,
        Input {
            axis: -0.9375,
            ..input()
        },
        Vec2::splat(-1.),
        false,
        0.016,
    );
    assert_eq!(host.controller().scroll(), 30.);
    host.prepare().unwrap();
    let mut frames = Frames::new(&gpu);
    let labels = frames.draw(&gpu, &host);
    let visible = std::mem::take(&mut host.visible);
    let panel = frames.draw(&gpu, &host);
    host.visible = visible;
    let mut changed = 0;
    let mut outside = 0;
    for (y, (a, b)) in labels
        .chunks_exact(320 * 4)
        .zip(panel.chunks_exact(320 * 4))
        .enumerate()
    {
        for (a, b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            if a != b {
                changed += 1;
                if !(40..160).contains(&y) {
                    outside += 1;
                }
            }
        }
    }
    assert!(
        changed > 40,
        "system-font chapter labels must reach the framebuffer"
    );
    assert_eq!(
        outside, 0,
        "native stencil y240..960 must clip all text fragments"
    );
    let red = |x: usize| panel[(100 * 320 + x) * 4];
    assert!(
        red(10) < red(60) && red(60) < red(80),
        "panel must retain left-to-right alpha gradient"
    );
    host.update(
        1.,
        Input {
            axis: 2.,
            ..input()
        },
        Vec2::splat(-1.),
        false,
        0.016,
    );
    host.prepare().unwrap();
    assert!(!host.visible.contains(&0));
    host.update(
        1.,
        Input {
            axis: -2.,
            ..input()
        },
        Vec2::splat(-1.),
        false,
        0.016,
    );
    host.prepare().unwrap();
    assert_eq!(host.visible.last(), Some(&0));
    let reverse = frames.draw(&gpu, &host);
    assert!(reverse.iter().any(|&b| b != 0));
    println!(
        "GER_BACKLOG_CHAPTER_HOST frames=3 text_pixels={changed} outside_clip={outside} font=system48 width=1274+6 cache_reversal=retained audio=Loopback Audio"
    );
}
