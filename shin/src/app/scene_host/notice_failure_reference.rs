//! Storage error owners with a valid system atlas missing only dialog artwork.
use crate::asset::system::{LayeredAssetIo, supplied_payload};
use std::path::PathBuf;

pub(super) fn override_notice(io: &mut LayeredAssetIo) -> PathBuf {
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_NOTICE_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let mut bytes = supplied_payload("/systex.txa").2;
    let at = bytes
        .windows(13)
        .position(|s| s == b"dialog_frame\0")
        .unwrap();
    // Same-size name mutation leaves all geometry, bodies and other entries valid.
    bytes[at + 11] = b'x';
    std::fs::write(directory.join("systex.txa"), bytes).unwrap();
    io.try_with_dir(&directory).unwrap();
    directory
}

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; temporary saves/assets only"]
fn reference_startup_storage_failure_without_dialog_artwork() {
    super::storage_read_reference::run_startup_storage_failure(true);
}

pub(super) use crate::adv::storage_notice_failure_reference::assert_notice_pixels;

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; temporary saves/assets only"]
fn reference_shutdown_storage_failure_without_dialog_artwork() {
    super::shutdown_reference::run_shutdown_failure(true);
}

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; temporary saves/assets only"]
fn reference_adv_storage_failure_without_dialog_artwork() {
    use crate::{
        adv::assets::AdvAssets,
        asset::system::{AssetLoadContext, AssetServer, cache::AssetCache},
        render::PreRenderContext,
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
    use std::sync::Arc;
    crate::init_test_task_pools();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    let _fixture = override_notice(&mut io);
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Storage notice fallback reference"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Storage notice fallback depth"),
        size: target.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ctx = PreRenderContext {
        device: &device,
        queue: &queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    crate::adv::storage_notice_failure_reference::verify(&server, &assets, &mut ctx, &target);
}
