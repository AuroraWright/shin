//! Reuse actual progress failure callbacks and check restored-scene notice ownership.
use super::{Adv, assets::AdvAssets};
use crate::{
    app::AppAction, asset::system::AssetServer, render::PreRenderContext, update::UpdateContext,
};
use shin_core::{primitives::update::FrameId, time::Ticks, vm::Scripter};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{path::PathBuf, sync::Arc, time::Instant};

pub(crate) fn assert_notice_pixels(pixels: &[u8], name: &str) {
    let white = pixels
        .chunks_exact(4)
        .filter(|p| p[..3].iter().all(|&v| v > 170))
        .count();
    assert!(
        white > 100,
        "{name}: readable notice glyphs missing ({white})"
    );
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_NOTICE_OUTPUT").unwrap());
    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
    for p in pixels.chunks_exact(4) {
        ppm.extend_from_slice(&p[..3]);
    }
    std::fs::write(output.join(format!("{name}.ppm")), ppm).unwrap();
    println!("{name}: plain frame and {white} bright glyph pixels visible");
}

pub(crate) fn verify<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    super::live_save_reference::assert_automatic_save_failure(server, assets, ctx, target, true);
    let mut live = Adv::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    let mut frame = FrameId::default();
    let step = |live: &mut Adv,
                ctx: &mut PreRenderContext<'a, '_, '_, '_>,
                frame: &mut FrameId,
                action: Option<AppAction>| {
        frame.advance();
        let mut input = enum_map::EnumMap::default();
        if let Some(action) = action {
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        live.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                pre_render: ctx,
            },
            input,
        );
        let encoder = std::mem::replace(
            ctx.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        let mut upload = ctx.device.create_command_encoder(&Default::default());
        ctx.dynamic_buffer.finish(&mut upload);
        ctx.queue.submit([upload.finish(), encoder.finish()]);
        ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while live.vm_state.save_position == 0 {
        assert!(Instant::now() < deadline);
        step(&mut live, ctx, &mut frame, None);
    }
    live.adv_state.acquire_audio_pause();
    live.adv_state.acquire_audio_pause();
    live.adv_state.acquire_ui_hide();
    let before_entry = serde_json::to_vec(&live.reference_entry()).unwrap();
    let before_save = live.save_menu_data().encode();
    live.show_scene_load_error(
        &anyhow::anyhow!("Fixture restored-scene resource failure"),
        server,
    )
    .unwrap();
    assert!(
        live.restore_error_dialog
            .as_ref()
            .unwrap()
            .reference_uses_plain_frame()
    );
    for _ in 0..30 {
        step(&mut live, ctx, &mut frame, None);
        assert_eq!(
            serde_json::to_vec(&live.reference_entry()).unwrap(),
            before_entry
        );
        assert_eq!(live.save_menu_data().encode(), before_save);
        assert_eq!(live.adv_state.audio_pause_count, 3);
    }
    let view = target.create_view(&Default::default());
    {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(ctx.depth_stencil),
            "Scene failure plain notice",
        );
        live.render(&mut pass);
    }
    let pixels = crate::layer::user::readback_test_texture(ctx, target);
    assert_notice_pixels(&pixels, "scene-restoration");
    step(&mut live, ctx, &mut frame, Some(AppAction::Enter));
    for _ in 0..40 {
        if live.restore_error_dialog.is_none() {
            break;
        }
        step(&mut live, ctx, &mut frame, None);
    }
    assert!(live.restore_error_dialog.is_none());
    assert_eq!(live.adv_state.audio_pause_count, 2);
    assert_eq!(live.adv_state.ui_hide_count, 1);
    assert_eq!(
        serde_json::to_vec(&live.reference_entry()).unwrap(),
        before_entry
    );
    assert_eq!(live.save_menu_data().encode(), before_save);
    println!(
        "Malformed restored-scene notice: same live entry/progress and foreign scopes retained; acknowledgement consumed"
    );
}
