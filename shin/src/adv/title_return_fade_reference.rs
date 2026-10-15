// Actual return confirmation, save notification, factory capture and90tick fade.
pub(super) fn assert_title_return_fade<'a>(
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    use crate::app::SceneHost;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    fn step<'a>(
        host: &mut SceneHost,
        server: &'a Arc<AssetServer>,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        frame: &mut FrameId,
        delta: f32,
        action: Option<AppAction>,
    ) {
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        if let Some(action) = action {
            input[action].is_clicked = true;
            input[action].is_clicked_or_repeated = true;
        }
        frame.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: ctx,
            },
            input,
            glam::vec2(1266.0, 700.0),
            glam::vec2(1900.0, 1050.0),
            (1280, 720),
            0.0,
            1.0 / 60.0,
        )
        .unwrap();
        let encoder = std::mem::replace(
            ctx.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        let mut upload = ctx.device.create_command_encoder(&Default::default());
        ctx.dynamic_buffer.finish(&mut upload);
        ctx.queue.submit([upload.finish(), encoder.finish()]);
        ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    }
    fn draw(
        host: &SceneHost,
        ctx: &mut PreRenderContext,
        target: &wgpu::Texture,
        capture: bool,
    ) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(ctx.depth_stencil),
                "Scenario return fade reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            if capture {
                host.render(&mut pass);
            } else {
                host.reference_title()
                    .unwrap()
                    .reference_render_without_return_capture(&mut pass);
            }
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    let folder =
        std::env::temp_dir().join(format!("shin-title-return-fade-{}", std::process::id()));
    assert!(!folder.exists());
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("progress.dat");
    let mut live = Adv::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    live.configure_save_file(path.clone(), None).unwrap();
    let mut host = SceneHost::new(live, assets.clone(), server.clone());
    let mut frame = FrameId::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while host.reference_adv().unwrap().vm_state.save_position == 0 {
        step(&mut host, server, context, &mut frame, 1.0, None);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    // Public pause-menu controls open the actual title confirmation and accept Yes.
    for (delta, action) in [
        (0.0, Some(AppAction::SystemMenu)),
        (22.0, None),
        (0.0, None),
        (0.0, Some(AppAction::Up)),
        (0.0, Some(AppAction::Enter)),
        (7.0, None),
        (0.0, None),
        (12.0, None),
        (0.0, None),
        (0.0, Some(AppAction::Left)),
        (0.0, Some(AppAction::Enter)),
        (7.0, None),
        (0.0, None),
        (12.0, None),
        (5.0, None),
    ] {
        step(&mut host, server, context, &mut frame, delta, action);
    }
    assert!(host.reference_adv().unwrap().pending_save.is_some());
    let position = host.reference_adv().unwrap().vm_state.save_position;
    while host.reference_adv().unwrap().pending_save.is_some() {
        assert!(
            host.reference_adv()
                .unwrap()
                .pending_title_setup()
                .is_none(),
            "Title must wait for save notification closure"
        );
        assert!(host.reference_title().is_none());
        step(&mut host, server, context, &mut frame, 1.0, None);
        assert_eq!(
            host.reference_adv().unwrap().vm_state.save_position,
            position
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        host.reference_adv()
            .unwrap()
            .pending_title_setup()
            .is_some()
    );
    let saved = std::fs::read(&path).unwrap();
    let outgoing = draw(&host, context, target, true);
    step(&mut host, server, context, &mut frame, 0.0, None);
    assert!(host.reference_adv().is_none());
    assert_eq!(
        host.reference_title().unwrap().reference_return_capture(),
        Some(1.0)
    );
    assert_eq!(
        draw(&host, context, target, true),
        outgoing,
        "Factory must retain the scene after the save notice closes"
    );
    while !host.reference_title().unwrap().is_loaded() {
        step(&mut host, server, context, &mut frame, 0.0, None);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        draw(&host, context, target, true),
        outgoing,
        "Asset publication must not flash the Title frame"
    );
    step(
        &mut host,
        server,
        context,
        &mut frame,
        45.0,
        Some(AppAction::Enter),
    );
    assert_eq!(
        host.reference_title().unwrap().reference_return_capture(),
        Some(0.5)
    );
    assert!(!host.reference_title().unwrap().motion.accepts_input());
    let middle = draw(&host, context, target, true);
    let underneath = draw(&host, context, target, false);
    let error = outgoing
        .chunks_exact(4)
        .zip(underneath.chunks_exact(4))
        .zip(middle.chunks_exact(4))
        .flat_map(|((a, b), m)| {
            (0..3).map(move |i| (m[i] as f32 - (a[i] as f32 + b[i] as f32) * 0.5).abs())
        })
        .fold(0.0_f32, f32::max);
    assert!(
        error <= 2.0,
        "Actual scene/Title midpoint blend error {error}"
    );
    step(&mut host, server, context, &mut frame, 45.0, None);
    assert_eq!(
        host.reference_title().unwrap().reference_return_capture(),
        Some(0.0)
    );
    let end = draw(&host, context, target, true);
    let changed = outgoing
        .chunks_exact(4)
        .zip(end.chunks_exact(4))
        .filter(|(a, b)| a[..3] != b[..3])
        .count();
    assert!(
        changed > 10000,
        "Scenario return must visibly fade into Title"
    );
    step(&mut host, server, context, &mut frame, 0.0, None);
    assert_eq!(
        host.reference_title().unwrap().reference_return_capture(),
        None
    );
    for _ in 0..250 {
        if host.reference_title().unwrap().motion.accepts_input() {
            break;
        }
        step(&mut host, server, context, &mut frame, 1.0, None);
    }
    assert!(host.reference_title().unwrap().motion.accepts_input());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        saved,
        "Transition must preserve just-saved progress"
    );
    println!(
        "Scenario return: actual Yes/save notification closure, retained outgoing frame,90tick fade midpoint error{error}, {changed} changed pixels, blocked input and eventual Title activation pass"
    );
    drop(host);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(folder).unwrap();
}
