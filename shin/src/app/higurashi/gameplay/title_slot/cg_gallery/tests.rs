//! Actual scene-slot globals/capture/retirement/retry boundaries. Frame/globals
//! helpers copied from the accepted SNRSEL suite; no previous suite is edited.
use super::*;
use crate::{
    adv::{
        higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
        play_clock::PlayClock,
    },
    app::higurashi::{
        gameplay::tests::{audio, fresh, policy},
        load_game::{Source, Storage},
        runtime_game_data::RuntimeGameData,
        runtime_saves::Slots,
        title_demo::TitleRequest,
    },
    audio::SystemSoundPlayer,
};
use shin_core::primitives::color::UnormColor;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[derive(Default)]
struct NoWrites(AtomicUsize);
impl Storage for NoWrites {
    fn trial_token(&self) -> bool {
        false
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn write(&self, _: &[u8]) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("CG plain return must not write progress")
    }
}
fn same_globals(slot: &TitleSlot, data: &SaveData, rows: &Slots) {
    let g = slot.globals().unwrap();
    let mut current = g.data.clone();
    // Native global clock and real BGM starts may change these two fields.
    current.play_seconds = data.play_seconds;
    current.save_vectors.unlocked_bgms = data.save_vectors.unlocked_bgms.clone();
    assert_eq!(current.encode().unwrap(), data.encode().unwrap());
    let current = g.runtime_saves.unwrap();
    for (current, expected) in std::iter::once(&current.auto)
        .chain(current.manual.iter())
        .zip(std::iter::once(&rows.auto).chain(rows.manual.iter()))
    {
        assert_eq!(current.valid, expected.valid);
        assert_eq!(
            current.data, expected.data,
            "complete two-record/four-vector rows were projected"
        );
    }
    assert!(g.platform_flag);
    assert_eq!(g.last_title_mode, 1);
}

#[test]
#[ignore = "actual Title/CG scenes,full globals/rows,capture/source-death retry,offscreen Metal/exact Loopback Audio;no window/user saves"]
fn higurashi_cg_scene_transfer_retains_full_rows_capture_and_constructor_retry() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = Arc::new(crate::app::higurashi::first_boot::CommonAssets {
        scenario: server.load_sync("/main.snr").unwrap(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    for size in [(1280, 720), (1920, 1080)] {
        let mut globals = load_globals(&server, common.clone());
        let writes = Arc::new(NoWrites::default());
        globals.storage = Some(writes.clone());
        globals.sounds.lock().bank = Some(common.sysse.clone());
        let mut slot = TitleSlot::new(
            TitleTransition {
                request: TitleRequest {
                    selected: 5,
                    mode: 1,
                },
                globals,
            },
            None,
        );
        let mut frames = Frames {
            number: 0,
            size,
            composed: None,
        };
        frames.step(&device, &queue, &mut slot, 3., 0);
        let deadline = Instant::now() + Duration::from_secs(15);
        while slot.title.as_ref().unwrap().reference_menu_state() != Some(("top", 2)) {
            frames.step(&device, &queue, &mut slot, 3., 0);
            assert!(Instant::now() < deadline, "source Title did not open");
        }
        assert_eq!(
            slot.title.as_ref().unwrap().reference_top_selection(),
            Some(5)
        );
        // Real Top5 -> gallery Category5 -> CG request, no direct pending edit.
        frames.step(&device, &queue, &mut slot, 3., 1 << 24);
        while slot.title.as_ref().unwrap().reference_category() != Some((1, 5, 2)) {
            frames.step(&device, &queue, &mut slot, 3., 0);
            assert!(Instant::now() < deadline, "gallery Category did not open");
        }
        frames.step(&device, &queue, &mut slot, 3., 1 << 24);
        while !matches!(slot.pending, Some(Pending::Cg)) {
            frames.step(&device, &queue, &mut slot, 3., 0);
            assert!(
                Instant::now() < deadline,
                "actual Category did not queue CG"
            );
        }
        let data = slot.globals().unwrap().data.clone();
        let rows = slot.globals().unwrap().runtime_saves.unwrap().clone();
        let source = frames.composed.as_ref().unwrap();
        let image = captured(&device, &queue, size, source);
        slot.reference_fail_next_constructor();
        reference_render_to_texture(&device, &queue, size, |ctx, _| {
            assert!(slot.dispatch_cg_gallery(ctx, source, (0, 0)).is_err());
            assert!(slot.title.is_some());
            assert!(matches!(slot.pending, Some(Pending::Cg)));
            assert!(
                slot.dispatch_cg_gallery(ctx, source, (size.0 as u16, size.1 as u16))
                    .is_err()
            );
        });
        assert_eq!(slot.active_scene().unwrap(), ActiveScene::RetiredFactory);
        assert!(slot.title.is_none() && slot.cg_gallery.is_none());
        same_globals(&slot, &data, &rows);
        let Some(Pending::PreparedCg(transfer)) = &slot.pending else {
            panic!("retained CG transfer")
        };
        assert_eq!(
            captured(&device, &queue, size, transfer.snapshot.as_ref().unwrap()),
            image
        );
        reference_render_to_texture(&device, &queue, size, |ctx, _| {
            assert!(
                slot.dispatch_cg_gallery(ctx, source, (size.0 as u16, size.1 as u16))
                    .unwrap()
            );
        });
        assert_eq!(slot.active_scene().unwrap(), ActiveScene::CgGallery);
        same_globals(&slot, &data, &rows);
        let before = slot.globals().unwrap().clock.ticks();
        frames.step(&device, &queue, &mut slot, 10., 0);
        assert_eq!(slot.globals().unwrap().clock.ticks(), before + 10.);
        let deadline = Instant::now() + Duration::from_secs(15);
        while !slot
            .cg_gallery
            .as_ref()
            .unwrap()
            .reference_owner()
            .reference_readiness()
            .2
        {
            frames.step(&device, &queue, &mut slot, 3., 0);
            assert!(Instant::now() < deadline, "actual CG First did not open");
        }
        same_globals(&slot, &data, &rows);
        frames.step(&device, &queue, &mut slot, 3., 1 << 25);
        while !matches!(slot.pending, Some(Pending::CgTitle(_))) {
            frames.step(&device, &queue, &mut slot, 3., 0);
            assert!(
                Instant::now() < deadline,
                "actual CG Back did not queue Title"
            );
        }
        assert!(matches!(
            slot.pending,
            Some(Pending::CgTitle(TitleRequest {
                selected: 15,
                mode: u32::MAX
            }))
        ));
        let returned = frames.composed.as_ref().unwrap();
        reference_render_to_texture(&device, &queue, size, |ctx, _| {
            assert!(slot.dispatch_cg_title(ctx, returned).unwrap());
        });
        assert!(slot.cg_gallery.is_none());
        assert_eq!(
            slot.title.as_ref().unwrap().reference_initial_selection(),
            (15, 1)
        );
        same_globals(&slot, &data, &rows);
        assert_eq!(writes.0.load(Ordering::SeqCst), 0);
    }
    println!(
        "CG scene:two actual Top5/Category5 requests,invalid preflight/source retained,two post-retirement constructor faults/retries with exact720p1080p source copies,all101 full runtime rows and complete globals retained,full10-tick clock,real First/Back/freshTitle15 resolvedmode1,zero storage writes; no window/user saves"
    );
}
fn load_globals(
    server: &Arc<AssetServer>,
    common: Arc<crate::app::higurashi::first_boot::CommonAssets>,
) -> Globals {
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut data = SaveData::default();
    data.persist_data.0 = vec![-1; 192];
    data.save_vectors.unlocked_cgs = vec![u32::MAX; 32];
    data.save_vectors.seen_messages = vec![0xa5; 4];
    data.save_vectors.select_seen_choices = vec![0x1234; 3];
    data.save_vectors.select_last_choices = vec![1, 2, 3, 4];
    data.save_vectors.extra_words_0 = vec![u32::MAX; 20];
    let mut slots = Slots::loaded(&data);
    slots.auto.valid = true;
    slots.auto.data = RuntimeGameData::from(fresh());
    slots.auto.data.progress.extra_word = 0xdeadbeef;
    slots.auto.data.progress.entries[1].response_data = vec![0x55, 0xf1];
    slots.manual[2].valid = true;
    slots.manual[2].data = slots.auto.data.clone();
    slots.manual[2].data.progress.set_active_index(1).unwrap();
    slots.manual[2].data.trailing_values = vec![0xffff, 0x1234];
    Globals {
        common: common.clone(),
        data,
        storage: None,
        runtime_saves: Some(slots),
        clock: PlayClock::from_saved_seconds(17),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), common.scenario.clone()),
        server: server.clone(),
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    }
}
struct Frames {
    number: u32,
    size: (u32, u32),
    composed: Option<RenderTexture>,
}
impl Frames {
    fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        pressed: u32,
    ) -> Vec<u8> {
        self.buttons(
            device,
            queue,
            slot,
            elapsed,
            Buttons {
                pressed,
                ..Buttons::default()
            },
        )
    }
    fn buttons(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        buttons: Buttons,
    ) -> Vec<u8> {
        self.number += 1;
        let mut composed = None;
        let image = reference_render_to_texture(device, queue, self.size, |context, target| {
            if let Some(previous) = &self.composed {
                shin_tasks::block_on(slot.update_frame(
                    context,
                    previous,
                    (self.size.0 as u16, self.size.1 as u16),
                    Ticks::from_f32(elapsed),
                    true,
                    buttons,
                    &PointerFrame {
                        number: self.number,
                        time_seconds: self.number as f64 / 60.0,
                        events: &[],
                    },
                    policy(),
                    audio(),
                ))
                .unwrap();
            }
            slot.prepare(context).unwrap();
            let mut frame = context.new_render_texture("Picker composed source".into());
            for output in [
                frame.as_texture_target(),
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass =
                    context.begin_pass(output, Some(context.depth_stencil), "Picker scene frame");
                pass.clear(
                    Some(UnormColor::from_rgba(31, 63, 127, 255)),
                    Some(0),
                    Some(1.0),
                );
                slot.render(&mut pass).unwrap();
            }
            composed = Some(frame);
        });
        self.composed = composed;
        image
    }
}
fn captured(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    source: &RenderTexture,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        let overlay = crate::adv::higurashi::runner::reconstruction_mode::Overlay {
            snapshot: source.copy_snapshot(&mut context.render_clone_ctx()),
            mask: None,
            clock: crate::adv::higurashi::reconstruction_mode::OverlayClock::captured(true),
        };
        let mut pass = context.begin_pass(
            shin_render::shaders::types::texture::TextureTarget {
                view: target,
                kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Picker source copy",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        overlay.render(&mut pass);
    })
}
