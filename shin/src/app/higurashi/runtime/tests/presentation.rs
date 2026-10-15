use super::*;

// The window supplies a depth/stencil attachment; the original Journey pixel
// helper did not. Exercise that difference through the production final copy.
fn present(
    gpu: &mut Graphics,
    source: &RenderTexture,
    extent: (u32, u32),
    stencil: bool,
) -> Vec<u8> {
    gpu.dynamic.recall();
    let descriptor = wgpu::TextureDescriptor {
        label: Some("Desktop presentation regression"),
        size: wgpu::Extent3d {
            width: extent.0,
            height: extent.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    let target = gpu.device.create_texture(&descriptor);
    let view = target.create_view(&Default::default());
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        ..descriptor
    });
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut context = gpu.context(&mut encoder);
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::Screen,
            },
            stencil.then_some(DepthStencilTarget { view: &depth_view }),
            "Desktop final copy with window stencil",
        );
        // Match the actual window's cleared stencil value, including the
        // closing-frame copy that renders into the composition target.
        pass.clear(
            Some(UnormColor::BLACK),
            stencil.then_some(0),
            stencil.then_some(1.),
        );
        Runtime::render_texture(source, &mut pass);
    }
    crate::layer::user::readback_test_texture(&mut context, &target)
}

#[test]
#[ignore = "Metal presentation with the desktop depth/stencil attachment at 720p,1080p and4K; no audio or progress writes"]
fn higurashi_desktop_presentation_matches_composition_with_window_stencil() {
    let (device, queue, _server) = crate::adv::higurashi::reference_gpu_assets();
    let mut gpu = Graphics::new(device, queue);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut context = gpu.context(&mut encoder);
    let mut composition = Composition::new(&mut context);
    context
        .begin_pass(
            composition.color.as_texture_target(),
            None,
            "Visible composed frame",
        )
        .clear(Some(UnormColor(0xff4b8bd0)), None, None);
    let mut uploads = gpu.device.create_command_encoder(&Default::default());
    gpu.dynamic.finish(&mut uploads);
    gpu.queue.submit([uploads.finish(), encoder.finish()]);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();

    for extent in [(1280, 720), (1920, 1080), (3840, 2160)] {
        let expected = present(&mut gpu, &composition.color, extent, false);
        assert!(
            expected
                .chunks_exact(4)
                .all(|pixel| pixel == [0xd0, 0x8b, 0x4b, 0xff])
        );
        let actual = present(&mut gpu, &composition.color, extent, true);
        assert!(
            actual == expected,
            "desktop stencil discarded the composed image at {extent:?}"
        );
        eprintln!(
            "Desktop composition visible and byte-identical with window stencil at {extent:?}"
        );
    }
}
