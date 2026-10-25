//! Unregistered concrete-owner checks. Parent runs these serialized, with the
//! audio device and supplied assets; this agent executes none of them.
//! Native model/parent/literal-Sprite acceptance remains independently required.
use super::*;
use crate::{
    adv::{
        higurashi::{reference_gpu_assets, reference_render_to_texture},
        input_area::{PointerEvent, PointerKind},
    },
    app::higurashi::first_boot::CommonAssets,
    asset::system::{AssetLoadContext, LayeredAssetIo, locate_assets_for_game},
    layer::user::higurashi::selected_asset_name,
};
use glam::IVec2;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

mod roster_eof;

#[derive(Default)]
struct Events {
    accepted: Vec<Action>,
    fail: Option<Action>,
}
impl Services for Events {
    fn event(&mut self, action: Action) -> Result<()> {
        if self.fail.as_ref() == Some(&action) {
            self.fail = None;
            bail!("controlled unacknowledged Movie parent service");
        }
        self.accepted.push(action);
        Ok(())
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
fn environment(
    server: &Arc<AssetServer>,
    common: &Arc<CommonAssets>,
    audio: &Arc<AudioManager>,
) -> Environment {
    Environment {
        common: common.clone(),
        server: server.clone(),
        sounds: Arc::new(Mutex::new(crate::audio::SystemSoundPlayer::new(
            audio.clone(),
        ))),
        storage: None,
    }
}
fn settings() -> SaveData {
    let mut data = SaveData::default();
    data.save_vectors.unlocked_movies = vec![u32::MAX; 8];
    data
}
fn pointer(number: u32) -> PointerFrame<'static> {
    PointerFrame {
        number,
        time_seconds: f64::from(number) / 60.,
        events: &[],
    }
}
fn until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "{label}");
        std::thread::yield_now();
    }
}
fn snapshot(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<RenderTexture> {
    let mut result = None;
    reference_render_to_texture(device, queue, (1280, 720), |ctx, _| {
        let mut texture = ctx.new_render_texture("Movie controlled prior composition".into());
        {
            let mut pass = ctx.begin_pass(
                texture.as_texture_target(),
                Some(ctx.depth_stencil),
                "Movie source bitmap fixture",
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
fn image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    extent: (u32, u32),
    owner: &Owner,
    data: &SaveData,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, extent, |ctx, target| {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(ctx.depth_stencil),
            "Movie actual retained owner",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.),
        );
        owner.render(data, &mut pass).unwrap();
    })
}
fn tick(
    owner: &mut Owner,
    events: &mut Events,
    frame: &mut FrameId,
    number: &mut u32,
    delta: f32,
    pressed: u32,
) -> Result<()> {
    frame.advance();
    *number += 1;
    owner.poll(
        Ticks::from_f32(delta),
        true,
        Buttons {
            pressed,
            ..Buttons::default()
        },
        &pointer(*number),
        *frame,
        events,
    )
}
fn ready_grid(owner: &mut Owner, events: &mut Events, frame: &mut FrameId, number: &mut u32) {
    until("Real First worker completed independently", || {
        owner.retirements().iter().all(|r| r.is_finished())
    });
    // Native956c0: completion observation, reveal, insertion, then prune.
    for _ in 0..8 {
        tick(owner, events, frame, number, 30., 0).unwrap();
        if owner.reference_state().3 && owner.loader.job.is_none() {
            return;
        }
    }
    panic!("Real Movie First did not retire into the grid Input task");
}

struct HeldDrop {
    atlas: std::sync::Weak<CharacterTextures>,
    mask: Option<std::sync::Weak<MaskTexture>>,
    send: mpsc::Sender<(bool, bool)>,
}
impl Drop for HeldDrop {
    fn drop(&mut self) {
        let _ = self.send.send((
            self.atlas.upgrade().is_some(),
            self.mask.as_ref().is_some_and(|m| m.upgrade().is_some()),
        ));
    }
}

#[test]
#[ignore = "Movie actual atlas/mask worker publication, pending/finished-unobserved cancellation,8 offscreen720p1080p frames; audio fixture; no playback/window/user saves"]
fn higurashi_movie_owner_publication_and_join_resource_order() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = common(&server);
    let audio = Arc::new(AudioManager::new_for_tests());
    let data = settings();
    let baseline = serde_json::to_vec(&data).unwrap();
    for at in [ReferencePublication::Atlas, ReferencePublication::Mask] {
        for finish_unobserved in [false, true] {
            let mut events = Events::default();
            let mut capture = snapshot(&device, &queue);
            let (published_tx, published_rx) = mpsc::channel();
            let (drop_tx, drop_rx) = mpsc::channel();
            let (release, held) = futures::channel::oneshot::channel();
            let mut release = Some(release);
            let mut owner = Owner::new_with_reference_gate(
                environment(&server, &common, &audio),
                audio.clone(),
                common.scenario.clone(),
                &data,
                &mut capture,
                (1280, 720),
                &mut events,
                at,
                Box::new(move |resources| {
                    Box::pin(async move {
                        let _drop = HeldDrop {
                            atlas: resources.atlas.clone(),
                            mask: resources.mask.clone(),
                            send: drop_tx,
                        };
                        published_tx.send(resources).unwrap();
                        let _ = held.await;
                    })
                }),
            )
            .unwrap();
            assert!(capture.is_none());
            let resources = published_rx
                .recv_timeout(Duration::from_secs(15))
                .expect("actual worker publication");
            assert_eq!(
                owner.reference_state(),
                (
                    true,
                    at == ReferencePublication::Mask,
                    false,
                    false,
                    true,
                    0
                )
            );
            assert!(resources.atlas.upgrade().is_some());
            assert!(owner.loader.job.as_ref().unwrap().task.is_some());
            let before = owner.model.frame;
            for extent in [(1280, 720), (1920, 1080)] {
                assert!(
                    image(&device, &queue, extent, &owner, &data)
                        .chunks_exact(4)
                        .any(|p| p[..3] != [0, 0, 0])
                );
                assert_eq!(owner.model.frame, before, "Rendering does not poll clocks");
            }
            let (mut frame, mut number) = (FrameId::default(), 0);
            tick(&mut owner, &mut events, &mut frame, &mut number, 0., 0).unwrap();
            assert!(!events.accepted.contains(&Action::StopBgm(60)));
            assert!(!owner.loader.completed);
            let retirement = owner.retirements();
            if finish_unobserved {
                release.take().unwrap().send(()).unwrap();
                until("Actual First output finished but not main-polled", || {
                    retirement.iter().all(|r| r.is_finished())
                });
                assert!(!owner.loader.completed);
            }
            drop(owner);
            drop(release);
            assert!(retirement.iter().all(|r| r.is_finished()));
            let (atlas_at_join, mask_at_join) = drop_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("held future was actually dropped");
            assert_eq!(atlas_at_join, finish_unobserved);
            assert_eq!(mask_at_join, at == ReferencePublication::Mask);
            assert!(resources.atlas.upgrade().is_none());
            assert!(
                resources
                    .mask
                    .as_ref()
                    .is_none_or(|m| m.upgrade().is_none())
            );
            assert_eq!(serde_json::to_vec(&data).unwrap(), baseline);
        }
    }
    eprintln!(
        "Movie owner:4 actual atlas/mask gates,8 pre-completion720p1080p parent frames,pending/unobserved-complete synchronous join,atlas release before private scope,mask retained until join,weak resources balanced; not native race/decoder/window/save proof"
    );
}

#[test]
#[ignore = "actual Movie First fallible parent-service prefix and synchronous fault retirement; audio fixture, no movie playback/window/user saves"]
fn higurashi_movie_owner_fault_is_latched_without_later_clocks_or_false_completion() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = common(&server);
    let audio = Arc::new(AudioManager::new_for_tests());
    let data = settings();
    let mut capture = snapshot(&device, &queue);
    let mut events = Events {
        fail: Some(Action::Achievement(36)),
        ..Events::default()
    };
    assert!(
        Owner::new_retained(
            environment(&server, &common, &audio),
            audio.clone(),
            common.scenario.clone(),
            &data,
            &mut capture,
            (1280, 720),
            &mut events,
        )
        .is_err()
    );
    assert!(
        capture.is_some(),
        "Failed construction cannot consume its source"
    );
    let mut owner = Owner::new_retained(
        environment(&server, &common, &audio),
        audio,
        common.scenario.clone(),
        &data,
        &mut capture,
        (1280, 720),
        &mut events,
    )
    .unwrap();
    until("First output ready", || {
        owner.retirements().iter().all(|r| r.is_finished())
    });
    let (mut frame, mut number) = (FrameId::default(), 0);
    events.fail = Some(Action::StopBgm(60));
    assert!(tick(&mut owner, &mut events, &mut frame, &mut number, 3., 0).is_err());
    assert_eq!(owner.model.frame, 0);
    assert!(!owner.reference_state().3);
    assert!(owner.scope.iter().any(|t| match &t.task {
        Task::First { phase, overlay } => *phase == 0 && overlay.active && overlay.elapsed == 0.,
        _ => false,
    }));
    let accepted = events.accepted.clone();
    assert!(
        tick(
            &mut owner,
            &mut events,
            &mut frame,
            &mut number,
            120.,
            1 << 25
        )
        .is_err()
    );
    assert_eq!(events.accepted, accepted);
    assert_eq!(owner.model.frame, 0);
    assert!(owner.fault.is_some());
    let retirement = owner.retirements();
    let resources = owner.reference_resources();
    drop(owner);
    assert!(retirement.iter().all(|r| r.is_finished()));
    assert!(resources.atlas.upgrade().is_none());
    assert!(
        resources
            .mask
            .as_ref()
            .is_none_or(|m| m.upgrade().is_none())
    );
    eprintln!(
        "Movie owner:construction failure retains capture; actual completed First starts native reveal prefix then failed StopBgm stops all later clocks/Input; fault remains explicit and drains resources, no native rollback or retry-success claim"
    );
}

/// Only the media files are overridden. Catalog,12-ID order,TXA,font,mask and
/// all owner/model/Player code remain actual. No fabricated Player is installed.
struct ShortMedia(PathBuf);
impl ShortMedia {
    fn new(audio: bool, scenario: &HigurashiScenario) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "shin-movie-gallery-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join("movie")).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../test-fixtures/higurashi/live-movie-owner-fixtures")
            .join(if audio {
                "short-aac.mp4"
            } else {
                "short-silent.mp4"
            });
        for info in &scenario.movie_info {
            let name = selected_asset_name(&info.name.to_string(), 0);
            std::fs::copy(&source, directory.join(format!("movie/{name}.mp4"))).unwrap();
        }
        Self(directory)
    }
    fn assets(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<AssetServer> {
        let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&self.0).unwrap();
        io.with(
            locate_assets_for_game(crate::cli::Game::Higurashi, &root)
                .unwrap()
                .into(),
        );
        Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Higurashi,
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ))
    }
}
impl Drop for ShortMedia {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn viewer(owner: &Owner) -> &Viewer {
    owner
        .scope
        .iter()
        .find_map(|t| match &t.task {
            Task::Viewer(v) => Some(v),
            _ => None,
        })
        .expect("retained native Viewer")
}
fn native_title_return() -> Vec<Action> {
    // Existing immutable original profile109. This is not a new original run,
    // codec oracle or fresh expected generation; strict rerun remains model_tests.
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/higurashi/movie-gallery-model-native-corpus/output.txt"
    ))
    .lines()
    .filter_map(|line| {
        let event: serde_json::Value = serde_json::from_str(line.strip_prefix("E ")?).unwrap();
        if event["profile"] != 109 {
            return None;
        }
        let words = event["words"].as_array().unwrap();
        let word = |i: usize| words[i].as_u64().unwrap() as u32;
        Some(match event["tag"].as_str().unwrap() {
            "achievement" => Action::Achievement(word(0)),
            "bgm-stop" => Action::StopBgm(word(0)),
            "cue" => Action::Cue(["move", "decide", "cancel", "error"][word(0) as usize]),
            "title-queue" => Action::ReturnTitle {
                selected: word(0) as i32,
                mode: word(1) as i32,
            },
            _ => return None,
        })
    })
    .collect()
}

#[test]
#[ignore = "actual Movie grid/private Viewer/Player with2 short silent/AAC fixtures,physical pointer,4 offscreen720p1080p frames;audio; no window/user saves"]
fn higurashi_movie_owner_real_viewer_returns_retained_grid_then_native_title17() {
    let (device, queue, base_server) = reference_gpu_assets();
    let base_server = Arc::new(base_server);
    let base_common = common(&base_server);
    let audio = Arc::new(AudioManager::new_for_tests());
    let data = settings();
    let baseline = serde_json::to_vec(&data).unwrap();
    let native = native_title_return();
    assert_eq!(native.len(), 5);
    for has_audio in [false, true] {
        let extent = if has_audio { (1920, 1080) } else { (1280, 720) };
        let fixture = ShortMedia::new(has_audio, &base_common.scenario);
        let server = fixture.assets(&device, &queue);
        let common = common(&server);
        let mut events = Events::default();
        let mut capture = snapshot(&device, &queue);
        let mut owner = Owner::new_retained(
            environment(&server, &common, &audio),
            audio.clone(),
            common.scenario.clone(),
            &data,
            &mut capture,
            extent,
            &mut events,
        )
        .unwrap();
        let (mut frame, mut number) = (FrameId::default(), 0);
        ready_grid(&mut owner, &mut events, &mut frame, &mut number);
        let atlas = Arc::as_ptr(&owner.loader.atlas().unwrap()) as usize;
        let resources = owner.reference_resources();
        assert_eq!(events.accepted.as_slice(), &native[..2]);
        assert_eq!(owner.model.selected(), 0);
        assert_eq!(owner.model.rows[0].movie_id, 9);
        // Actual physical720p/1080p InputArea produces canonical150,225.
        // Down alone changes selection and captures; Up opens the actual file.
        let point = [PointerEvent {
            kind: PointerKind::Down,
            id: 17,
            position: if extent.0 == 1280 {
                IVec2::new(100, 150)
            } else {
                IVec2::new(150, 225)
            },
        }];
        frame.advance();
        number += 1;
        owner
            .poll(
                Ticks::ZERO,
                true,
                Buttons::default(),
                &PointerFrame {
                    number,
                    time_seconds: number as f64 / 60.,
                    events: &point,
                },
                frame,
                &mut events,
            )
            .unwrap();
        assert!(owner.media.is_empty());
        let point = [PointerEvent {
            kind: PointerKind::Up,
            id: 17,
            position: IVec2::ZERO,
        }];
        frame.advance();
        number += 1;
        owner
            .poll(
                Ticks::ZERO,
                true,
                Buttons::default(),
                &PointerFrame {
                    number,
                    time_seconds: number as f64 / 60.,
                    events: &point,
                },
                frame,
                &mut events,
            )
            .unwrap();
        assert_eq!(owner.media.len(), 1);
        let id = viewer(&owner).id;
        assert_eq!(viewer(&owner).backdrop.value(), 0.);
        assert!(!owner.media[&id].player.as_ref().unwrap().is_playing());
        let mut borrowed_viewers = 0;
        owner
            .scope
            .draw_ref(|t| borrowed_viewers += usize::from(matches!(t.task, Task::Viewer(_))));
        assert_eq!(
            borrowed_viewers, 0,
            "New Viewer is absent from current borrowed draw snapshot"
        );
        until("Actual short Player prepared independently", || {
            owner.media[&id].player.as_ref().unwrap().status() != 0
        });
        assert_eq!(owner.media[&id].player.as_ref().unwrap().status(), 1);
        // Native Player+58 can present/convert while locally paused. Prepare
        // its retained frame BEFORE external suspension: that gate correctly
        // suppresses the entire transport update, including first conversion.
        until(
            "Actual paused short Player converted retained frame",
            || {
                tick(&mut owner, &mut events, &mut frame, &mut number, 0., 0).unwrap();
                owner
                    .reference_viewer_state()
                    .unwrap()
                    .retained_frame
                    .is_some()
            },
        );
        assert!(!owner.media[&id].player.as_ref().unwrap().is_playing());
        // Explicit external transport suspension keeps these short files at the
        // first frame while real GPU work runs. It changes no native task clock.
        audio.set_suspended(true);
        owner.sync_suspension();
        struct Resume<'a>(&'a AudioManager);
        impl Drop for Resume<'_> {
            fn drop(&mut self) {
                self.0.set_suspended(false);
            }
        }
        let resume = Resume(&audio);
        tick(&mut owner, &mut events, &mut frame, &mut number, 30., 0).unwrap();
        assert!(!owner.media[&id].player.as_ref().unwrap().is_playing());
        tick(&mut owner, &mut events, &mut frame, &mut number, 0., 0).unwrap();
        assert!(owner.media[&id].player.as_ref().unwrap().is_playing());
        assert!(
            viewer(&owner)
                .scope
                .iter()
                .any(|t| matches!(t, super::super::ViewerTask::Input { done: false }))
        );
        assert_eq!(
            owner.reference_viewer_state().unwrap().retained_frame,
            Some((64, 48))
        );
        let before = owner.model.frame;
        for extent in [(1280, 720), (1920, 1080)] {
            assert!(
                image(&device, &queue, extent, &owner, &data)
                    .chunks_exact(4)
                    .any(|p| p[..3] != [0, 0, 0])
            );
            assert_eq!(owner.model.frame, before);
        }
        assert_eq!(Arc::as_ptr(&owner.loader.atlas().unwrap()) as usize, atlas);
        tick(
            &mut owner,
            &mut events,
            &mut frame,
            &mut number,
            0.,
            1 << 25,
        )
        .unwrap();
        assert!(viewer(&owner).closing);
        tick(&mut owner, &mut events, &mut frame, &mut number, 30., 0).unwrap();
        assert_eq!(viewer(&owner).opacity.value(), 0.);
        assert!(owner.media[&id].player.as_ref().unwrap().is_playing());
        tick(&mut owner, &mut events, &mut frame, &mut number, 0., 0).unwrap();
        assert!(!owner.media[&id].player.as_ref().unwrap().is_playing());
        assert_eq!(viewer(&owner).backdrop.value(), 1.);
        for _ in 0..8 {
            tick(&mut owner, &mut events, &mut frame, &mut number, 30., 0).unwrap();
            if owner.media.is_empty() {
                break;
            }
        }
        assert!(
            owner.media.is_empty(),
            "Viewer stop/release/private scope/InputArea retire before grid resumes"
        );
        assert!(owner.reference_state().3 && !owner.model.closing);
        assert_eq!(Arc::as_ptr(&owner.loader.atlas().unwrap()) as usize, atlas);
        assert!(
            !events
                .accepted
                .iter()
                .any(|a| matches!(a, Action::ReturnTitle { .. }))
        );
        assert_eq!(serde_json::to_vec(&data).unwrap(), baseline);
        drop(resume);
        // Separate parent close: compare only its five exact external native
        // profile109 actions (construction prefix2 above, return suffix3 here).
        let close_start = events.accepted.len();
        tick(
            &mut owner,
            &mut events,
            &mut frame,
            &mut number,
            0.,
            1 << 25,
        )
        .unwrap();
        for _ in 0..8 {
            tick(&mut owner, &mut events, &mut frame, &mut number, 120., 0).unwrap();
            if events
                .accepted
                .iter()
                .any(|a| matches!(a, Action::ReturnTitle { .. }))
            {
                break;
            }
        }
        assert_eq!(&events.accepted[close_start..], &native[2..]);
        assert_eq!(serde_json::to_vec(&data).unwrap(), baseline);
        drop(owner);
        assert!(resources.atlas.upgrade().is_none());
    }
    eprintln!(
        "Movie owner:2 actual short silent/AAC Player/private-scope lifecycles,physical row0 tap,new-child snapshot delay,separate opacity/backdrop exit,retained atlas/grid,4 actual720p1080p frames,10 external actions from frozen native profile109,unchanged full settings; controlled external suspension,no real window/SDK codec equality/category1 bus claim"
    );
}
