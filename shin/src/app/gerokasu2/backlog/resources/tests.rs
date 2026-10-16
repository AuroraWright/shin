use super::*;
use crate::{
    app::gerokasu2::tests::{assets, bytes},
    asset::font::gerokasu2::draw::gpu_tests::Offscreen,
    render::PreRenderContext,
};
use shin_core::format::save::gerokasu2::{Progress, SaveDate};
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
    target: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    count: usize,
}
impl Frames {
    fn new(gpu: &Offscreen) -> Self {
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Ger backlog composed rows"),
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
                label: Some("Ger backlog test depth"),
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
            view: target.create_view(&Default::default()),
            target,
            depth,
            resize: SurfaceResizeSource::new(ViewportParams::both((320, 180).into())),
            samplers: TextureSamplerStore::new(&gpu.device),
            pipelines: PipelineStorage::new(gpu.device.clone(), shin_render::TEXTURE_FORMAT),
            dynamic: DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(2 * 1024 * 1024)),
            count: 0,
        }
    }
    fn draw(
        &mut self,
        gpu: &Offscreen,
        resources: &Resources,
        presentation: &Presentation,
        flashes: &[f32],
    ) -> Vec<u8> {
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
                "Ger backlog row resources",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            resources
                .render_rows(&mut pass, presentation, flashes)
                .unwrap();
            resources.render_chrome(&mut pass, presentation);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut ctx, &self.target);
        self.dynamic.recall();
        self.count += 1;
        pixels
    }
}
fn history() -> History {
    use backlog::history::{Entry, Item};
    let entries = (0..16)
        .map(|i| Entry {
            boundary: i + 2,
            item: match i {
                4 => Item::Choice {
                    title: "第一章".into(),
                    text: "右を選ぶ".into(),
                    response: 1,
                },
                8 => Item::Checkpoint {
                    title: "第二章".into(),
                    text: "記録した見出し".into(),
                },
                _ => Item::Message {
                    message_id: i,
                    message_arg: (i % 4) as i32,
                    text: format!(
                        "名前@r「日本語の履歴、行{i}。@{{太字@}}と@c900.赤@c999.。」@o37.@vfixture."
                    ),
                },
            },
        })
        .collect();
    let mut progress = Progress::fresh(0, 42);
    progress.active_entry_mut().save_position = 10;
    progress.active_entry_mut().replay_target = 13;
    History {
        entries,
        chapters: vec![4, 8],
        current_row: Some(6),
        jump_source: GameData {
            date: SaveDate::default(),
            progress,
        },
    }
}
fn different(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
#[ignore = "supplied Ger atlases/fonts, Japanese row glyphs, overscan and memory payload on Metal/exact Loopback Audio"]
fn ger_backlog_rows_render_text_portraits_chrome_and_retain_native_cache_order() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut resources = shin_tasks::block_on(Resources::from_history(
        assets.clone(),
        history(),
        cancelled.clone(),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(resources.history.entries.len(), 16);
    assert_eq!(resources.geometry.rows[0].height, 232.);
    let mut model = resources.controller().unwrap();
    assert_eq!(model.selected(), 6);
    let mut view = Presentation {
        progress: 1.,
        scroll: model.scroll(),
        selected: model.selected(),
        chapter_open: false,
        swapped: false,
        close_flash: 0.,
        game: crate::cli::Game::Gerokasu2,
    };
    resources.prepare(view.scroll).unwrap();
    let original_cache = resources.visible.clone();
    assert!(original_cache.len() >= 4);
    let mut frames = Frames::new(&gpu);
    let normal = frames.draw(&gpu, &resources, &view, &[]);
    let text = std::mem::take(&mut resources.text);
    resources.text = std::iter::repeat_with(|| None).take(16).collect();
    let without_text = frames.draw(&gpu, &resources, &view, &[]);
    resources.text = text;
    let text_pixels = different(&normal, &without_text);
    assert!(
        text_pixels > 40,
        "Japanese SDF glyph passes did not affect the composed frame: {text_pixels}"
    );
    let flashing = frames.draw(&gpu, &resources, &view, &vec![0.7; 16]);
    let portrait_pixels = different(&normal, &flashing);
    assert!(
        portrait_pixels > 20,
        "native portrait flash did not affect the composed frame"
    );
    assert_eq!(
        model.input(backlog::Input {
            permission: true,
            repeated: 1 << 24,
            ..Default::default()
        }),
        [backlog::Event::Move]
    );
    for _ in 0..4 {
        model.advance_scroll();
    }
    view.selected = model.selected();
    let selection = frames.draw(&gpu, &resources, &view, &[]);
    assert!(different(&normal, &selection) > 20);
    view.progress = 0.25;
    let opening = frames.draw(&gpu, &resources, &view, &[]);
    assert!(different(&normal, &opening) > 1000);
    view.progress = 1.;
    view.scroll = resources.geometry.maximum;
    resources.prepare(view.scroll).unwrap();
    assert!(
        original_cache
            .iter()
            .any(|i| !resources.visible.contains(i))
    );
    frames.draw(&gpu, &resources, &view, &[]);
    view.scroll = model.scroll();
    resources.prepare(view.scroll).unwrap();
    assert!(
        resources
            .visible
            .iter()
            .all(|&i| resources.text[i].is_some())
    );
    let revisited = frames.draw(&gpu, &resources, &view, &[]);
    assert_eq!(
        selection, revisited,
        "evicted glyph owners must rebuild the same visible frame"
    );
    let source = resources.history.jump_source.clone();
    let jump = resources.jump_data(4).unwrap();
    assert_eq!(jump.progress.active_entry().save_position, 6);
    assert_eq!(jump.progress.active_entry().replay_target, 10);
    assert_eq!(resources.history.jump_source, source);
    assert!(resources.jump_data(99).is_err());
    let empty = GameData {
        date: SaveDate::default(),
        progress: Progress::fresh(0, 42),
    };
    assert!(
        shin_tasks::block_on(Resources::load(
            assets.clone(),
            scenario.clone(),
            empty.clone(),
            SaveData::default(),
            cancelled.clone()
        ))
        .unwrap()
        .is_none()
    );
    cancelled.store(true, Ordering::Release);
    assert!(
        shin_tasks::block_on(Resources::load(
            assets,
            scenario,
            empty,
            SaveData::default(),
            cancelled
        ))
        .is_err()
    );
    println!(
        "GER_BACKLOG_ROW_RESOURCES rows=16 frames={} text_changed_pixels={text_pixels} portrait_changed_pixels={portrait_pixels} atlas_NewRodin_SDF_fixed220_column_overscan_eviction_revisit_memory_jump_cancel=verified audio=Loopback Audio",
        frames.count
    );
}
