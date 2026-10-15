use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use crate::app::higurashi::first_boot::CommonAssets;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    cell::Cell,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Events {
    accepted: Vec<Action>,
    fail: Option<Action>,
    reads: Cell<usize>,
}
impl Services for Events {
    fn event(&mut self, action: Action) -> Result<()> {
        if self.fail.as_ref() == Some(&action) {
            self.fail = None;
            bail!("controlled unacknowledged Music service");
        }
        self.accepted.push(action);
        Ok(())
    }
    fn bgm_status(&self) -> u32 {
        self.reads.set(self.reads.get() + 1);
        2
    }
}
fn common(server: &Arc<AssetServer>) -> Arc<CommonAssets> {
    Arc::new(CommonAssets {
        scenario: server.load_sync("/main.snr").unwrap(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    })
}
fn env(server: &Arc<AssetServer>, common: &Arc<CommonAssets>) -> Environment {
    // Environment uses the exact-device fixture; all Music actions use Events
    // above, so this check performs no actual Music playback.
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    Environment {
        common: common.clone(),
        server: server.clone(),
        sounds: Arc::new(Mutex::new(crate::audio::SystemSoundPlayer::new(manager))),
        storage: None,
    }
}
fn pointer(number: u32) -> PointerFrame<'static> {
    PointerFrame {
        number,
        time_seconds: f64::from(number) / 60.,
        events: &[],
    }
}
fn settings() -> SaveData {
    let mut data = SaveData::default();
    data.save_vectors.unlocked_bgms = vec![u32::MAX; 8];
    data
}
fn image(device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32), owner: &Owner) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |ctx, target| {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(ctx.depth_stencil),
            "Music real owner publication",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.),
        );
        owner.render(&mut pass).unwrap();
    })
}
fn capture(device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) -> Option<RenderTexture> {
    let mut result = None;
    reference_render_to_texture(device, queue, size, |ctx, _| {
        let mut texture = ctx.new_render_texture("Music controlled whole-source bitmap".into());
        {
            let mut pass = ctx.begin_pass(
                texture.as_texture_target(),
                Some(ctx.depth_stencil),
                "Music source fixture",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::from_rgba(
                    71, 109, 137, 255,
                )),
                Some(0),
                Some(1.),
            );
        }
        result = Some(texture);
    });
    result
}
fn poll(
    owner: &mut Owner,
    events: &mut Events,
    delta: f32,
    pressed: u32,
    number: u32,
) -> Result<()> {
    owner.poll(
        Ticks::from_f32(delta),
        true,
        Buttons {
            pressed,
            ..Buttons::default()
        },
        &pointer(number),
        events,
    )
}
fn wait_finished(owner: &Owner) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !owner.retirements().iter().all(|r| r.is_finished()) {
        assert!(
            Instant::now() < deadline,
            "actual Music loader did not finish"
        );
        std::thread::yield_now();
    }
}

#[test]
#[ignore = "actual Music loader/atlas/mask/glyphs/offscreen720p1080p,independent publication,pending/finished-unobserved join;exact Loopback fixture,no playback/window/user saves"]
fn higurashi_music_owner_publications_render_before_completion_and_cancel_join() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = common(&server);
    for at in [ReferencePublication::Atlas, ReferencePublication::Mask] {
        for finish_unobserved in [false, true] {
            let (mut events, mut snapshot) =
                (Events::default(), capture(&device, &queue, (1280, 720)));
            let (tx, rx) = mpsc::channel();
            let (release, held) = futures::channel::oneshot::channel();
            let mut release = Some(release);
            let mut owner = Owner::new_with_reference_gate(
                env(&server, &common),
                &common.scenario,
                &settings(),
                &mut snapshot,
                (1280, 720),
                &mut events,
                at,
                Box::new(move |resources| {
                    Box::pin(async move {
                        tx.send(resources).unwrap();
                        let _ = held.await;
                    })
                }),
            )
            .unwrap();
            assert!(snapshot.is_none());
            let resources = rx
                .recv_timeout(Duration::from_secs(15))
                .expect("actual worker publication");
            assert!(resources.atlas.upgrade().is_some());
            assert_eq!(
                owner.reference_state(),
                (
                    true,
                    at == ReferencePublication::Mask,
                    false,
                    false,
                    true,
                    false
                )
            );
            let bar = owner.model().scrollbar;
            assert_eq!(
                (bar.position, bar.visible, bar.first, bar.end),
                (0, 1, 0, 1)
            );
            let before = owner.model().elapsed.to_bits();
            for size in [(1280, 720), (1920, 1080)] {
                assert!(
                    image(&device, &queue, size, &owner)
                        .chunks_exact(4)
                        .any(|p| p[..3] != [0, 0, 0])
                );
                assert_eq!(owner.model().elapsed.to_bits(), before);
            }
            poll(&mut owner, &mut events, 0., 0, 1).unwrap();
            assert!(!events.accepted.contains(&Action::StopBgm(60)));
            assert_eq!(owner.reference_state().2, false);
            let retirement = owner.retirements();
            if finish_unobserved {
                release.take().unwrap().send(()).unwrap();
                wait_finished(&owner);
            }
            // Do not main-poll the completed output. The actual descendant scope
            // must still retire before capture/mask/job/parent atlas destruction.
            drop(owner);
            drop(release);
            assert!(retirement.iter().all(|r| r.is_finished()));
            assert!(resources.atlas.upgrade().is_none());
            assert!(
                resources
                    .mask
                    .as_ref()
                    .is_none_or(|m| m.upgrade().is_none())
            );
        }
    }
    println!(
        "Music owner:four actual atlas/mask gates,pending/finished-unobserved cancellation,8 pure pre-poll720p1080p art frames,valid native initial scrollbar,completion/Input gated,balanced weak resources;no playback/window/save"
    );
}

#[test]
#[ignore = "actual Music First/Input/Close fallible service ordering and retained retry,real asset/capture/Loopback fixture;no playback/window/user saves"]
fn higurashi_music_owner_service_failures_resume_exact_task_and_clock_suffix() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = common(&server);
    let mut snapshot = capture(&device, &queue, (1920, 1080));
    let mut events = Events {
        fail: Some(Action::Achievement(34)),
        ..Events::default()
    };
    assert!(
        Owner::new_retained(
            env(&server, &common),
            &common.scenario,
            &settings(),
            &mut snapshot,
            (1920, 1080),
            &mut events
        )
        .is_err()
    );
    assert!(snapshot.is_some());
    let mut owner = Owner::new_retained(
        env(&server, &common),
        &common.scenario,
        &settings(),
        &mut snapshot,
        (1920, 1080),
        &mut events,
    )
    .unwrap();
    wait_finished(&owner);
    events.fail = Some(Action::StopBgm(60));
    assert!(poll(&mut owner, &mut events, 3., 0, 1).is_err());
    assert_eq!(owner.pending_event(), Some(&Action::StopBgm(60)));
    assert_eq!(owner.model().elapsed, 0.);
    let first = owner
        .scope
        .iter()
        .find_map(|t| {
            if let Task::First { phase, overlay } = &t.task {
                Some((*phase, overlay.elapsed))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(first, (0, 0.));
    poll(&mut owner, &mut events, 99., 1 << 25, 2).unwrap();
    assert_eq!(owner.model().elapsed, 3. * f32::from_bits(0x3c888889));
    assert!(!owner.model().closing);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut number = 3;
    while !owner.reference_state().3 || owner.loader.job.is_some() {
        poll(&mut owner, &mut events, 3., 0, number).unwrap();
        number += 1;
        assert!(Instant::now() < deadline, "real Music First did not prune");
    }
    let old = owner.model().elapsed;
    let row = &owner.model().rows[0];
    let play = Action::PlayBgm {
        id: row.bgm,
        once: row.once,
        fade: 0,
        gain: 1.,
    };
    events.fail = Some(play.clone());
    assert!(poll(&mut owner, &mut events, 3., 1 << 24, number).is_err());
    number += 1;
    assert_eq!(owner.pending_event(), Some(&play));
    assert_eq!(owner.model().elapsed, old);
    assert_eq!(
        events.reads.get(),
        0,
        "status must wait for play acknowledgment"
    );
    poll(&mut owner, &mut events, 99., 1 << 25, number).unwrap();
    number += 1;
    assert_eq!(events.accepted.iter().filter(|a| **a == play).count(), 1);
    assert_eq!(events.reads.get(), 1);
    assert!(!owner.model().closing);
    assert_eq!(owner.model().elapsed, old + 3. * f32::from_bits(0x3c888889));
    owner.model.as_mut().unwrap().scroll.fast_forward_to(100.);
    owner.model.as_mut().unwrap().velocity = 4.;
    let old = owner.model().elapsed;
    events.fail = Some(Action::PlatformByte);
    assert!(poll(&mut owner, &mut events, 2., 0, number).is_err());
    number += 1;
    assert_eq!(owner.model().scroll.value(), 96.);
    assert_eq!(owner.model().scrollbar.position, 96);
    assert_eq!(owner.model().elapsed, old);
    let velocity = owner.model().velocity.to_bits();
    let reads = events.reads.get();
    poll(&mut owner, &mut events, 123., 1 << 25, number).unwrap();
    number += 1;
    assert_eq!(owner.model().velocity.to_bits(), velocity);
    assert_eq!(events.reads.get(), reads);
    assert_eq!(owner.model().elapsed, old + 2. * f32::from_bits(0x3c888889));
    assert!(!owner.model().closing);
    events.fail = Some(Action::ReturnTitle {
        selected: 16,
        mode: -1,
    });
    poll(&mut owner, &mut events, 3., 1 << 25, number).unwrap();
    number += 1;
    while !owner.pending_frame() {
        let result = poll(&mut owner, &mut events, 3., 0, number);
        number += 1;
        if result.is_err() {
            break;
        }
        assert!(number < 100, "Music Close did not reach Title request");
    }
    assert_eq!(
        owner.pending_event(),
        Some(&Action::ReturnTitle {
            selected: 16,
            mode: -1
        })
    );
    assert_eq!(
        events
            .accepted
            .iter()
            .filter(|a| **a == Action::StopBgm(30))
            .count(),
        1
    );
    poll(&mut owner, &mut events, 999., 1 << 24, number).unwrap();
    assert_eq!(
        events
            .accepted
            .iter()
            .filter(|a| **a == Action::StopBgm(30))
            .count(),
        1
    );
    assert_eq!(
        events
            .accepted
            .iter()
            .filter(|a| **a
                == Action::ReturnTitle {
                    selected: 16,
                    mode: -1
                })
            .count(),
        1
    );
    assert!(!events.accepted.contains(&Action::DisableInput));
    println!(
        "Music real owner:Achievement capture-retention,First Stop60 before phase/clock,Input Play before status,PlatformByte before tail clocks,Close Stop30 prefix beforeTitle16;four retained callback retries keep original delta/input snapshot,zero successful-prefix replay;no playback/window/save"
    );
}
