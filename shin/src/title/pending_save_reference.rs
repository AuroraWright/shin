//! Actual retained Title Config child with disk/volatile/failing SaveGame owners.
use super::{TitleItem, child::TitleChild};
use crate::{
    adv::{GlobalData, config::ConfigPhase, input_area::PointerFrame},
    app::AppAction,
    asset::system::AssetServer,
    render::PreRenderContext,
};
use shin_core::{
    format::save::{GameData, SaveData},
    time::Ticks,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) fn reference_pending_config_save<'a>(
    server: &'a Arc<AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    for (disk, failure) in [(false, false), (true, false), (true, true)] {
        let folder = std::env::temp_dir().join(format!(
            "shin-title-config-pending-{}-{disk}-{failure}",
            std::process::id()
        ));
        assert!(!folder.exists());
        let path = folder.join("progress.dat");
        let mut saved = SaveData::default();
        saved.settings.v3_sysvol = 35;
        saved.manual_save_slots[99] = Some(GameData::from_entry(
            shin_core::format::save::GameDataEntry {
                scenario_id: 7,
                random_seed: 42,
                save_position: 1,
                route_data: Default::default(),
            },
        ));
        saved.save_menu_position = 99;
        let mut globals =
            GlobalData::open(disk.then(|| path.clone()), Some(saved.clone())).unwrap();
        let mut child =
            TitleChild::new(TitleItem::Config, server.clone(), &globals, None, 0).unwrap();
        child.resize((1920, 1080));
        let mut number = 0;
        let mut step = |child: &mut TitleChild,
                        globals: &mut GlobalData,
                        delta: f32,
                        action: Option<AppAction>| {
            let mut input = enum_map::EnumMap::default();
            if let Some(action) = action {
                input[action] = shin_input::ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            let frame = PointerFrame {
                number,
                time_seconds: number as f64 / 60.0,
                events: &[],
            };
            number += 1;
            child.input(
                &frame,
                &input,
                glam::Vec2::ZERO,
                false,
                0.0,
                0.0,
                (1920, 1080),
            );
            let finished = child.update(Ticks::from_f32(delta), globals).unwrap();
            globals.advance_play_clock(Ticks::from_f32(delta));
            child.prepare(context);
            // Submit each prepared frame as App does. Pending workers must not
            // exhaust Metal's finite command-buffer pool while their UI is polled.
            let encoder = std::mem::replace(
                context.encoder,
                context.device.create_command_encoder(&Default::default()),
            );
            let mut upload = context.device.create_command_encoder(&Default::default());
            context.dynamic_buffer.finish(&mut upload);
            context.queue.submit([upload.finish(), encoder.finish()]);
            context.device.poll(wgpu::Maintain::Wait);
            finished
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.config().and_then(|screen| screen.model()).is_none() {
            assert!(!step(&mut child, &mut globals, 0.0, None));
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        step(&mut child, &mut globals, 10.0, None);
        assert_eq!(
            child.config().unwrap().model().unwrap().phase(),
            ConfigPhase::Active
        );
        step(&mut child, &mut globals, 0.0, Some(AppAction::Right));
        assert_eq!(globals.save().settings.v3_sysvol, 40);
        if failure {
            std::fs::write(&folder, b"Blocked Title save directory").unwrap();
        }
        step(&mut child, &mut globals, 7.0, Some(AppAction::Cancel));
        step(&mut child, &mut globals, 0.0, None);
        assert_eq!(
            child.config().unwrap().model().unwrap().phase(),
            ConfigPhase::WaitingSave
        );
        assert!(
            !path.exists(),
            "Title Config cannot write before the updating dialog opens"
        );
        step(&mut child, &mut globals, 12.0, None);
        step(&mut child, &mut globals, 0.0, None);
        let started = Instant::now();
        // Parent must retain the child and ignore edits/cancel while saving.
        for action in [AppAction::Right, AppAction::Cancel, AppAction::Enter] {
            assert!(!step(&mut child, &mut globals, 0.0, Some(action)));
            assert_eq!(globals.save().settings.v3_sysvol, 40);
            assert_eq!(
                child.config().unwrap().model().unwrap().phase(),
                ConfigPhase::WaitingSave
            );
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.config().unwrap().model().unwrap().phase() == ConfigPhase::WaitingSave {
            let acknowledgement = matches!(&child, super::child::TitleChild::Config { save: Some(save), .. } if save.reference_failure_is_open());
            assert!(!step(
                &mut child,
                &mut globals,
                1.0,
                Some(if acknowledgement {
                    AppAction::Enter
                } else {
                    AppAction::Right
                })
            ));
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        if !failure {
            assert!(started.elapsed() >= Duration::from_millis(990));
        }
        assert_eq!(
            child.config().unwrap().model().unwrap().phase(),
            ConfigPhase::Closing
        );
        assert_eq!(globals.save().settings.v3_sysvol, 40);
        if disk && !failure {
            let written = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(written.settings.v3_sysvol, 40);
            assert_eq!(written.save_menu_position, 99);
            assert!(written.manual_save_slots[99].is_some());
        }
        assert!(step(&mut child, &mut globals, 10.0, None));
        assert!(child.is_finished());
        drop(step);
        // Use the already prepared real child to prove its owner can retire.
        let _ = target;
        drop(child);
        drop(globals);
        if failure {
            assert_eq!(
                std::fs::read(&folder).unwrap(),
                b"Blocked Title save directory"
            );
            std::fs::remove_file(&folder).unwrap();
        } else if disk {
            std::fs::remove_dir_all(&folder).unwrap();
        }
        eprintln!(
            "Title Config pending SaveGame: disk{disk}, failure{failure}, retained child/input, captured globals/manual slots, worker timing, final callback and close0/1 pass"
        );
    }
}
