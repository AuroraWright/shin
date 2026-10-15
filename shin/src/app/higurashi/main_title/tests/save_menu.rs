use super::*;
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio, input::Buttons, reference_gpu_assets,
            reference_render_to_texture,
        },
        input_area::PointerFrame,
    },
    app::higurashi::{first_boot::CommonAssets, runtime_saves::Slots},
    audio::SystemSoundPlayer,
};
use parking_lot::Mutex;
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveData, SaveDate};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

fn poll(title: &mut Opening, number: &mut u32, pressed: u32, repeat: u32) {
    *number += 1;
    title
        .update_with_input(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                pressed,
                repeat,
                ..Default::default()
            },
            &PointerFrame {
                number: *number,
                time_seconds: f64::from(*number) / 60.0,
                events: &[],
            },
        )
        .unwrap();
}
fn until(title: &mut Opening, number: &mut u32, ready: impl Fn(&Opening) -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ready(title) {
        poll(title, number, 0, 0);
        assert!(Instant::now() < end, "Title SaveMenu boundary timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
#[ignore = "real retained Title Top/SaveMenu/Question children,Back stage1/return_id1 blackout and snapshot ordering,full Load flags0 without story90/BGM-stop at720p/1080p on offscreen Metal/exact Loopback Audio;no window/writes"]
fn higurashi_title_manual_menu_back_and_load_preserve_native_parent_order() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = Arc::new(CommonAssets {
        scenario: server.load_sync("/main.snr").unwrap(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let mut profiles = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut saved = SaveData::default();
        saved.save_menu_position = 2;
        saved.manual_save_slots[2] = Some(GameData {
            date: SaveDate {
                year: 2025,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
            },
            extra_word: 7,
            entry: GameDataEntry {
                scenario_id: 100,
                random_seed: 42,
                save_position: 0,
                replay_target: 0,
                response_data: vec![0x23, 0xee],
            },
            primary_entry: None,
            trailing_values: vec![0xffff, 0x1234],
            byte_variables: vec![7, 255],
        });
        let mut slots = Slots::loaded(&saved);
        slots.manual[2].data.progress.extra_word = 0xdeadbeef;
        slots.manual[2].data.progress.entries[1] = crate::adv::higurashi::autosave::ProgressEntry {
            scenario_id: 2084,
            random_seed: 0x87654321,
            save_position: 17,
            replay_target: 19,
            response_data: vec![255, 1, 2],
        };
        let loaded = slots.manual[2].data.clone();
        let expected = slots.clone();
        let globals = Globals {
            common: common.clone(),
            data: saved,
            storage: None,
            runtime_saves: Some(slots),
            clock: crate::adv::play_clock::PlayClock::from_saved_seconds(17),
            platform_flag: true,
            last_title_mode: 7,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), common.scenario.clone()),
            server: server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
        };
        let mut title = Opening::new(
            TitleTransition {
                request: crate::app::higurashi::title_demo::TitleRequest {
                    selected: 1,
                    mode: 0,
                },
                globals,
            },
            None,
        );
        title.resize_menu_input((size.0 as u16, size.1 as u16));
        let mut number = 0;
        until(&mut title, &mut number, |t| t.top.is_some());
        for _ in 0..60 {
            poll(&mut title, &mut number, 0, 0);
        }
        for round in 0..2 {
            poll(&mut title, &mut number, 1 << 24, 0);
            until(&mut title, &mut number, |t| t.save.is_some());
            assert!(
                !title.save_visible,
                "New SaveMenu excluded from constructor-frame snapshot"
            );
            assert_eq!(title.save.as_ref().unwrap().0, 1);
            assert_eq!(title.save.as_ref().unwrap().1.model.progress, 0.0);
            assert_eq!(title.motion.clocks[4].value().to_bits(), 0.1f32.to_bits());
            until(&mut title, &mut number, |t| {
                t.save
                    .as_ref()
                    .is_some_and(|(_, m)| m.model.progress == 1.0)
            });
            poll(&mut title, &mut number, 0, 0);
            assert!(title.save_visible);
            assert!(title.top.is_none());
            assert_eq!(title.motion.clocks[4].value(), 0.5);
            let pixels = reference_render_to_texture(&device, &queue, size, |context, target| {
                title.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Title manual menu",
                );
                title.render(&mut pass);
            });
            assert!(pixels.chunks_exact(4).any(|p| p != [0, 0, 0, 255]));
            if round == 0 {
                poll(&mut title, &mut number, 1 << 25, 0);
                until(&mut title, &mut number, |t| {
                    t.save.as_ref().is_some_and(|(_, m)| m.model.closing)
                });
                assert!(title.top.is_none(), "Return0 must not create Top");
                until(&mut title, &mut number, |t| {
                    t.save.as_ref().is_some_and(|(_, m)| m.complete())
                });
                assert!(title.top.is_some());
                assert!(!title.top_visible);
                assert!(title.save_visible);
                let mut black = HigurashiTweener::new(0.5);
                black
                    .control(0.0, Ticks::from_f32(f32::from_bits(0x3dcccccd)), 0x40)
                    .unwrap();
                black.update(Ticks::from_f32(1.0));
                assert_eq!(
                    title.motion.clocks[4].value().to_bits(),
                    black.value().to_bits()
                );
                poll(&mut title, &mut number, 0, 0);
                assert!(title.save.is_none());
                assert!(!title.save_visible);
                assert!(title.top_visible);
                for _ in 0..60 {
                    poll(&mut title, &mut number, 0, 0);
                }
            } else {
                poll(&mut title, &mut number, 1 << 24, 0);
                until(&mut title, &mut number, |t| {
                    t.save.as_ref().unwrap().1.reference_question() == Some((2, 1))
                });
                poll(&mut title, &mut number, 0, 1 << 7);
                assert_eq!(
                    title.save.as_ref().unwrap().1.reference_question(),
                    Some((2, 0))
                );
                poll(&mut title, &mut number, 1 << 24, 0);
                assert!(title.manual_load.is_none());
                until(&mut title, &mut number, |t| t.manual_load.is_some());
                let (data, flags) = title.take_manual_load().unwrap();
                assert_eq!(flags, 0);
                assert_eq!(data, loaded);
                assert!(title.story_gate.is_none());
                assert!(title.top.is_none());
                assert!(!title.save.as_ref().unwrap().1.complete());
                assert_eq!(title.motion.clocks[4].value(), 0.5);
                let actual = title.globals.runtime_saves.as_ref().unwrap();
                for (a, e) in actual.manual.iter().zip(&expected.manual) {
                    assert_eq!(a.valid, e.valid);
                    assert_eq!(a.data, e.data);
                }
            }
            profiles += 1;
        }
    }
    eprintln!(
        "Title manual SaveMenu:{profiles} Back/Load profiles at720p/1080p,actual Top return_id1 constructor/blackout ordering,retained borrowed snapshots,real Question/final Load full records/flags0,no story90 gate or global-slot mutation pass"
    );
}
