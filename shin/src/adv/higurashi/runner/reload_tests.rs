use super::super::{
    audio::HigurashiAudio,
    frame::tests::gpu::render_to_texture,
    host::{HigurashiHost, system_menu::tests::hold_reload},
    input::Buttons,
    layers::Target,
    message_glyphs::tests::gpu::gpu_assets,
    read_history::{MessageReadHistory, ReadHistoryServices},
    read_state::ReadPolicy,
    scene_textures::HigurashiSceneTextures,
};
use super::*;
use crate::adv::input_area::PointerFrame;
use crate::audio::SystemSoundPlayer;
use binrw::BinWrite;
use parking_lot::Mutex;
use shin_core::format::scenario::instruction_elements::UntypedNumberSpec;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::time::{Duration, Instant};

fn press_close(host: &mut HigurashiHost) {
    host.poll_modes_with_input(
        Ticks::ZERO,
        1,
        Buttons {
            pressed: 1 << 15,
            ..Default::default()
        },
        &PointerFrame {
            number: 999,
            time_seconds: 0.0,
            events: &[],
        },
    )
    .unwrap();
}

#[test]
#[ignore = "real PC/CS PIC/BUP reload, retained modal owner and cancellation on offscreen Metal/exact Loopback Audio"]
fn higurashi_host_reload_publishes_selected_assets_without_manual_callback() {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let picture = authored
        .picture_info
        .iter()
        .position(|i| i.name.as_str().contains('?'))
        .unwrap() as i32;
    let bustup = authored
        .bustup_info
        .iter()
        .position(|i| i.name.as_str().contains('?'))
        .unwrap() as i32;
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut frames = 0;
    for (size, setting) in [((1280, 720), 1), ((1920, 1080), 0)] {
        let mut code = Vec::new();
        for (id, kind, params) in [
            (7, 2, vec![picture]),
            (9, 3, vec![bustup]),
            (11, 1, vec![65535, 0, 0, 1280, 720]),
        ] {
            code.extend([0xc1, id, kind, 0, ((1u16 << params.len()) - 1) as u8]);
            for value in params {
                let mut bytes = std::io::Cursor::new(Vec::new());
                UntypedNumberSpec::Constant(value)
                    .write_le(&mut bytes)
                    .unwrap();
                code.extend(bytes.into_inner());
            }
        }
        code.extend([
            0xa0, 1, 2, b'T', 0, 0xa1, 0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0, 0, 0,
        ]);
        let scenario = super::super::replay::tests::scenario(&code);
        let runner = shin_tasks::block_on(HigurashiRunner::load(
            assets.clone(),
            scenario.clone(),
            0,
            42,
            || [1, 2, 3, 4],
        ))
        .unwrap();
        let audio = HigurashiAudio::new(manager.clone(), assets.clone(), authored.clone());
        let mut host = HigurashiHost::new(runner, audio);
        host.set_menu_frame(1 - setting, 120.0);
        let mut reads = ReadHistoryServices::new(MessageReadHistory::new(), |_, _| Ok(()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while host
            .runner
            .native_save_metadata()
            .is_none_or(|m| m.saved()[1].as_ref() != "T")
        {
            assert!(
                Instant::now() < deadline,
                "Native LAYERLOAD did not finish: pending {:?}, metadata {:?}, completed {}",
                host.runner.pending.as_ref().map(|p| &p.command),
                host.runner.native_save_metadata(),
                host.runner.completed_commands()
            );
            shin_tasks::block_on(host.runner.pump_native(
                super::tests::policy(),
                ReadPolicy {
                    modes: 0,
                    forced: false,
                    read_only: false,
                },
                &mut reads,
                8,
                1000,
                || [1; 4],
                &mut host.audio,
            ))
            .unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        for id in [7, 9, 11] {
            host.runner
                .frame
                .scene
                .layers_mut()
                .unwrap()
                .state
                .control(id, 0, 123, 0)
                .unwrap();
        }
        let layers = host.runner.frame.scene.layers_mut().unwrap();
        for id in [7, 9] {
            shin_tasks::block_on(layers.dispatch(
                &Cmd::LAYERCTRL {
                    args: [id, 0],
                    params: (256, 120, 0, 0, 0, 0, 0, 0),
                },
                &assets,
                &scenario,
                || [0; 4],
            ))
            .unwrap();
            layers.advance(Ticks::from_f32(3.0), None);
        }
        // The back PIC is a property-only copy; the BUP back is a separate load.
        layers.state.start_back(7).unwrap();
        layers.state.control(7, 1, 333, 0).unwrap();
        layers.state.start_back(9).unwrap();
        let plan = layers
            .plan_load(9, 3, 0, (bustup, 0, 0, 0, 0, 0, 0, 0))
            .unwrap();
        let loaded =
            shin_tasks::block_on(plan.load_with_setting(&assets, &scenario, 1 - setting)).unwrap();
        layers.publish_load(loaded, || [21, 22, 23, 24]).unwrap();
        let front_targets = [7, 9, 11].map(|id| {
            let mut target = layers.state.target(id).unwrap();
            target.bank = 0;
            target
        });
        let properties =
            front_targets.map(|t| format!("{:?}", layers.layer(t).unwrap().properties));
        let back_target = layers.state.target(9).unwrap();
        let back_properties = format!("{:?}", layers.layer(back_target).unwrap().properties);
        let integer_state = format!("{:?}", layers.state);
        render_to_texture(&device, &queue, size, |context, _| {
            let mut screens = HigurashiSceneTextures::new(context);
            screens
                .prepare_scene(context, &host.runner.frame.scene)
                .unwrap();
            shin_tasks::block_on(host.open_system_menu(11001, sounds.clone(), context, &screens))
                .unwrap();
        });
        for _ in 0..10 {
            host.poll_modes(Ticks::from_f32(3.0), 1).unwrap();
        }
        host.set_menu_frame(setting, 120.0);
        press_close(&mut host);
        let gate = hold_reload(&mut host);
        for _ in 0..12 {
            host.poll_modes(Ticks::from_f32(3.0), 1).unwrap();
        }
        assert_eq!(host.modal_pause_depth(), 1);
        assert!(host.has_modes());
        assert!(host.take_menu_child_requests().is_empty());
        assert_eq!(
            format!("{:?}", host.runner.frame.scene.layers().state),
            integer_state
        );
        gate.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.has_modes() {
            assert!(Instant::now() < deadline, "Reload did not complete");
            host.poll_modes(Ticks::from_f32(3.0), 1).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(host.modal_pause_depth(), 0);
        assert!(host.take_menu_child_requests().is_empty());
        let layers = host.runner.frame.scene.layers();
        assert_eq!(format!("{:?}", layers.state), integer_state);
        for (target, expected) in front_targets.into_iter().zip(properties) {
            assert_eq!(
                format!("{:?}", layers.layer(target).unwrap().properties),
                expected
            );
        }
        assert_eq!(
            format!("{:?}", layers.layer(back_target).unwrap().properties),
            back_properties
        );
        let copy = Target {
            bank: 1,
            ..front_targets[0]
        };
        assert_eq!(
            format!("{:?}", layers.layer(copy).unwrap().properties),
            format!("{:?}", layers.layer(front_targets[0]).unwrap().properties)
        );
        for (target, name) in [
            (
                front_targets[0],
                authored.picture_info[picture as usize].name.as_str(),
            ),
            (
                front_targets[1],
                authored.bustup_info[bustup as usize].name.as_str(),
            ),
        ] {
            let name = crate::layer::user::higurashi::selected_asset_name(name, setting);
            assert!(
                format!("{:?}", layers.layer(target).unwrap()).contains(&name),
                "Selected asset label {name}"
            );
        }
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            let mut screens = HigurashiSceneTextures::new(context);
            screens
                .prepare_scene(context, &host.runner.frame.scene)
                .unwrap();
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "reloaded Higurashi scene",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            screens.render_runner(&mut pass, &host.runner).unwrap();
        });
        assert_eq!(pixels.len(), size.0 as usize * size.1 as usize * 4);
        assert!(pixels.chunks_exact(4).any(|p| p != [0, 0, 0, 255]));
        frames += 1;
        // Native destructor cancels/joins without acknowledging menu closure.
        host.set_menu_frame(setting, 120.0);
        render_to_texture(&device, &queue, size, |context, _| {
            let mut screens = HigurashiSceneTextures::new(context);
            screens
                .prepare_scene(context, &host.runner.frame.scene)
                .unwrap();
            shin_tasks::block_on(host.open_system_menu(11001, sounds.clone(), context, &screens))
                .unwrap();
        });
        for _ in 0..10 {
            host.poll_modes(Ticks::from_f32(3.0), 1).unwrap();
        }
        host.set_menu_frame(1 - setting, 120.0);
        press_close(&mut host);
        let gate = hold_reload(&mut host);
        let start = Instant::now();
        drop(host);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(gate.send(()).is_err());
    }
    println!(
        "Higurashi live LayerReloader:2 real PIC/BUP PC/CS host profiles with native VM LAYERLOAD, {frames} scene frames at720p/1080p, dirty copy/load, retained queues/seeds/VM, automatic closing callback and pending-task retirement; exact Loopback Audio"
    );
}
