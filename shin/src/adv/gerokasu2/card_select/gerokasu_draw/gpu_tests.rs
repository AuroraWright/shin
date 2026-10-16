//! Offscreen frames of the Gerokasu CardSelect for visual review. Writes
//! debug-capture `.rgba` dumps (see shin-window debug_capture) to
//! `SHIN_GEROKASU_CARD_FRAMES`; convert with `gerokasu/tools/rgba2png.py`.
use super::*;
use crate::{
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, LayeredAssetIo},
    },
    render::PreRenderContext,
};
use shin_core::time::Ticks;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};

const W: u32 = 1920;
const H: u32 = 1080;

fn assets(gpu: &Offscreen) -> Arc<AssetServer> {
    let rom = std::env::var_os("SHIN_GEROKASU_ROM").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../gerokasu/data.rom"),
        std::path::PathBuf::from,
    );
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(&rom).unwrap();
    Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ))
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    resize: SurfaceResizeSource,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
}
impl Target {
    fn new(gpu: &Offscreen) -> Self {
        let descriptor = |format, usage| wgpu::TextureDescriptor {
            label: Some("Ger CardSelect review frame"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        };
        let texture = gpu.device.create_texture(&descriptor(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        ));
        let depth = gpu.device.create_texture(&descriptor(
            shin_render::DEPTH_STENCIL_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        Self {
            view: texture.create_view(&Default::default()),
            texture,
            depth: depth.create_view(&Default::default()),
            resize: SurfaceResizeSource::new(ViewportParams::both((W, H).into())),
            samplers: TextureSamplerStore::new(&gpu.device),
            pipelines: PipelineStorage::new(gpu.device.clone(), wgpu::TextureFormat::Rgba8Unorm),
            dynamic: DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(4 * 1024 * 1024)),
        }
    }
    fn draw(&mut self, gpu: &Offscreen, layer: &GerokasuCardRenderer) -> Vec<u8> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
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
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &self.view,
                },
                Some(context.depth_stencil),
                "Ger CardSelect review",
            );
            // A mid-grey backdrop shows the effect's blend.
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::from_rgba(
                    90, 70, 70, 255,
                )),
                Some(0),
                Some(1.0),
            );
            layer.render_drawable_direct(
                &mut pass,
                &TransformParams::default(),
                &DrawableParams {
                    color_multiplier: shin_core::primitives::color::FloatColor4::WHITE,
                    blend_type: shin_render::LayerBlendType::Type1,
                    fragment_shader: shin_render::LayerFragmentShader::Default,
                    shader_param: glam::Vec4::ZERO,
                },
                &DrawableClipParams {
                    mode: crate::layer::render_params::DrawableClipMode::None,
                    area: glam::Vec4::ZERO,
                },
                0,
                PassKind::Transparent,
            );
        }
        crate::layer::user::readback_test_texture(&mut context, &self.texture)
    }
}

fn save(dir: &std::path::Path, name: &str, pixels: &[u8]) {
    let mut data = b"RGBA".to_vec();
    data.extend(W.to_le_bytes());
    data.extend(H.to_le_bytes());
    data.extend_from_slice(pixels);
    std::fs::write(dir.join(format!("{name}.rgba")), data).unwrap();
}

#[test]
#[ignore = "requires gerokasu/data.rom, Metal and exact Loopback Audio; writes review frames"]
fn gerokasu_card_select_review_frames() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("SHIN_GEROKASU_CARD_FRAMES").expect("SHIN_GEROKASU_CARD_FRAMES"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    // fu->ka, ka->mi and do->fu were already played.
    let mask = 0b10_0001_0001;
    let resources = shin_tasks::block_on(GerokasuCardResources::load(&assets, mask)).unwrap();
    let layer = construct(&resources, mask, 12345);
    let renderer = layer.inner_ref();
    let mut target = Target::new(&gpu);
    let step = |frames: usize| {
        for _ in 0..frames {
            let mut m = renderer.model.lock();
            m.update(Ticks::from_f32(1.0));
            if m.phase == 5 {
                m.voice_playing = false;
            }
        }
    };
    let mut shot = |name: &str| save(&dir, name, &target.draw(&gpu, renderer));
    shot("00-constructed");
    renderer.model.lock().attach_listener();
    step(25);
    shot("01-dealing");
    step(120);
    shot("02-hand");
    {
        let mut m = renderer.model.lock();
        let i = m.hand.iter().position(|c| c.kind == 0).unwrap() as i32;
        m.selected = i;
    }
    shot("03-focus-prisoner");
    renderer.model.lock().confirm();
    step(10);
    shot("04-selecting");
    step(60);
    shot("05-choose-slot");
    {
        // Fuuka (slot 0) was already prisoner once; doremi pierrot of fu done.
        let mut m = renderer.model.lock();
        m.slot = 0;
        m.confirm();
    }
    step(15);
    shot("06-assigning");
    step(40);
    shot("07-cutin");
    step(200);
    shot("08-placed-redealt");
    {
        let mut m = renderer.model.lock();
        let i = m.hand.iter().position(|c| c.kind == 1).unwrap() as i32;
        m.selected = i;
        m.confirm();
    }
    step(80);
    {
        // Karin as pierrot of fuuka is done (bit 0): rejected with a message.
        let mut m = renderer.model.lock();
        m.slot = 2;
        m.confirm();
    }
    step(30);
    shot("09-rejected-message");
    // Play the rest, taking the first slot each card is accepted on.
    for _ in 0..3000 {
        {
            let mut m = renderer.model.lock();
            if m.phase < 0 || m.phase == 7 {
                break;
            }
            if m.ready() && m.phase == 3 {
                let free: Vec<_> = (0..4)
                    .filter(|&s| m.placed.iter().all(|p| p.slot != s as usize))
                    .collect();
                for s in free {
                    m.slot = s;
                    m.confirm();
                    if m.phase != 3 {
                        break;
                    }
                }
            } else if m.ready() && m.phase == 1 {
                m.selected = 0;
                m.confirm();
            }
        }
        step(1);
    }
    step(40);
    shot("10-success-effect");
    step(400);
    shot("11-finished");
    let m = renderer.model.lock();
    assert!(m.result().is_some(), "phase {}", m.phase);
}
