//! Gerokasu dialog paint with the `dialog_logo` strip, dumped for review as a
//! debug-capture `.rgba` to `SHIN_GEROKASU_DIALOG_FRAME`.
use super::*;
use crate::asset::{
    font::gerokasu2::draw::gpu_tests::Offscreen,
    system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};

#[test]
#[ignore = "requires gerokasu/data.rom, Metal and exact Loopback Audio; writes a review frame"]
fn gerokasu_dialog_reserves_and_paints_logo_strip() {
    let out = std::env::var_os("SHIN_GEROKASU_DIALOG_FRAME").map(std::path::PathBuf::from);
    let gpu = Offscreen::new();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../gerokasu/data.rom"))
        .unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let mut dialog =
        GerDialog::load(&server, "ロードします。よろしいですか？", &["はい", "いいえ"], 0, true, false)
            .unwrap();
    let height = dialog.prompt.size.y;
    assert_eq!(dialog.logo, Some(height));
    // Native 21be8: buttons start 28+8 below the prompt.
    assert_eq!(dialog.geometry.buttons[0].y, height + 36.);
    let resize = SurfaceResizeSource::new(ViewportParams::both((1920, 1080).into()));
    let samplers = TextureSamplerStore::new(&gpu.device);
    let mut pipelines = PipelineStorage::new(gpu.device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(1024 * 1024));
    let depth = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Ger dialog review depth"),
            size: wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
        device: &gpu.device,
        queue: &gpu.queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    dialog.prepare(&mut context).unwrap();
    let pixels = crate::layer::user::readback_test_texture(&mut context, &dialog._cache);
    let [w, h] = [dialog._cache.width(), dialog._cache.height()];
    // The logo strip is opaque somewhere inside (50..270, H+30..H+58).
    let y = (height + 44.) as u32;
    let row = &pixels[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
    assert!((50..270).any(|x| row[x * 4 + 3] > 0));
    if let Some(out) = out {
        let mut data = b"RGBA".to_vec();
        data.extend(w.to_le_bytes());
        data.extend(h.to_le_bytes());
        data.extend_from_slice(&pixels);
        std::fs::write(out, data).unwrap();
    }
}
