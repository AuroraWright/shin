use super::*;
use crate::{
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, AssetServer, LayeredAssetIo},
    },
    update::AdvUpdateContext,
};
use shin_core::primitives::update::FrameId;
use shin_render::{RenderRequestBuilder, gpu_texture::GpuTexture};
use std::sync::Arc;

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn all_transition_requests_acquire_assets_update_and_render() {
    let mut gpu = Offscreen::new();
    let mut scenario = scenario();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(&std::path::PathBuf::from(
        std::env::var_os("SHIN_GEROKASU2_ROM").unwrap(),
    ))
    .unwrap();
    let assets = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    for id in 0..scenario.mask_info.len() {
        let request =
            TransitionRequest::resolve(&scenario, 1, 60, [id as i32, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(matches!(
            shin_tasks::block_on(request.load(&assets)).unwrap(),
            AnyWiper::Mask(_)
        ));
    }
    let source_color = [210u8, 40, 90, 255];
    let target_color = [20u8, 170, 230, 255];
    let texture = |color: [u8; 4]| {
        GpuTexture::new_static_with_data(
            &gpu.device,
            &gpu.queue,
            Some("Ger transition solid source"),
            (8, 8).into(),
            wgpu::TextureFormat::Rgba8Unorm,
            &color.repeat(64),
        )
    };
    let source = texture(source_color);
    let target = texture(target_color);
    let mut frames = 0;
    let mut channels = 0;
    for kind in 0..=9 {
        let params = match kind {
            1 => [0, 400, 7, 0, 0, 0, 0, 0],
            2 => [0, 2, 0, 0, 0, 0, 0, 0],
            3 => [1000, 500, 0, 0, 0, 0, 0, 0],
            6 => [2, 0, 0, 0, 0, 0, 0, 0],
            _ => [0; 8],
        };
        let request = TransitionRequest::resolve(&scenario, kind, 60, params).unwrap();
        let mut wipe = shin_tasks::block_on(request.load(&assets)).unwrap();
        let mut frame = FrameId::default();
        frame.advance();
        assert!(wipe.is_running());
        for (delta, allowed, progress) in [
            (100.0, false, 0.0),
            (15.0, true, 0.25),
            (30.0, true, 0.75),
            (0.0, true, 1.0),
        ] {
            if progress == 1.0 {
                wipe.fast_forward();
            }
            frame.advance();
            let context = AdvUpdateContext {
                frame_id: frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: &assets,
                device: &gpu.device,
                queue: &gpu.queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            };
            wipe.update(&context);
            wipe.update(&context); // Duplicate frame must not advance either clock/effect.
            assert_eq!(wipe.is_running(), progress < 1.0, "kind{kind}");
            if let AnyWiper::Default(w) = &wipe {
                assert_eq!(
                    w.reference_time_words(),
                    [(progress * 60.0f32).to_bits(), 60.0f32.to_bits()]
                );
            }
            let pixels = gpu.draw(|pass| {
                wipe.render(
                    pass,
                    RenderRequestBuilder::new(),
                    target.as_source(),
                    source.as_source(),
                )
            });
            frames += 1;
            channels += pixels.len() * 4;
            if kind == 0 {
                for pixel in pixels {
                    for channel in 0..4 {
                        let expected = source_color[channel] as f32 * (1.0 - progress)
                            + target_color[channel] as f32 * progress;
                        assert!(
                            (pixel[channel] as f32 - expected).abs() <= 1.0,
                            "default blend {pixel:?}/{progress}"
                        );
                    }
                }
            }
        }
    }
    let fallback = TransitionRequest::resolve(&scenario, 1, 60, [-1; 8]).unwrap();
    assert!(matches!(
        shin_tasks::block_on(fallback.load(&assets)).unwrap(),
        AnyWiper::Default(_)
    ));
    let supplied = scenario.mask_info.len();
    scenario.mask_info.push(MaskInfoItem {
        name: U16Utf8String::new("欠落マスク😀"),
    });
    let missing =
        TransitionRequest::resolve(&scenario, 1, 60, [supplied as i32, 500, 0, 0, 0, 0, 0, 0])
            .unwrap();
    assert!(
        shin_tasks::block_on(missing.load(&assets)).is_err(),
        "valid metadata with missing asset must retain the resource error"
    );
    assert!(TransitionRequest::resolve(&scenario, 10, 60, [0; 8]).is_none());
    println!(
        "GEROKASU2_TRANSITION_GPU kinds=10 supplied_masks={supplied} frames={frames} channels={channels} actual_assets_shared_wipers_pause_duplicate_frame_fast_forward_default_blend_metadata_fallback_resource_error=verified audio=default output"
    );
}
