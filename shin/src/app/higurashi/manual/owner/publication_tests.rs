//! Real Manual loader scheduling/ownership checks. Root runs these serialized
//! with supplied assets, offscreen GPU and EXACT Loopback Audio. Rendering here
//! exercises publication/purity; frozen native packet/literal-shader evidence
//! remains the independent math oracle. Native progress0 may be offscreen.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    app::higurashi::{
        first_boot::CommonAssets,
        load_game::{Source, Storage},
    },
    audio::SystemSoundPlayer,
};
use shin_core::primitives::color::UnormColor;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    ops::{Deref, DerefMut},
    sync::{
        Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct NoWrites(AtomicUsize);
impl Storage for NoWrites {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        anyhow::bail!("Manual must not read progress")
    }
    fn trial_exists(&self) -> Result<bool> {
        anyhow::bail!("Manual must not probe trial saves")
    }
    fn write(&self, _: &[u8]) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("Manual must not write progress")
    }
}
struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    env: Environment,
    writes: Arc<NoWrites>,
    _audio: Arc<shin_audio::AudioManager>,
}
impl Fixture {
    fn new() -> Self {
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
        let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut sounds = SystemSoundPlayer::new(audio.clone());
        sounds.bank = Some(common.sysse.clone());
        let writes = Arc::new(NoWrites::default());
        Self {
            device,
            queue,
            env: Environment {
                common,
                server,
                sounds: Arc::new(Mutex::new(sounds)),
                storage: Some(writes.clone()),
            },
            writes,
            _audio: audio,
        }
    }
    fn environment(&self) -> Environment {
        Environment {
            common: self.env.common.clone(),
            server: self.env.server.clone(),
            sounds: self.env.sounds.clone(),
            storage: self.env.storage.clone(),
        }
    }
    fn image(&self, owner: &Owner, extent: (u32, u32)) -> Vec<u8> {
        let before = observation(owner);
        let image =
            reference_render_to_texture(&self.device, &self.queue, extent, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(ctx.depth_stencil),
                    "Manual independent publication observation",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                owner.render(&mut pass);
            });
        assert_eq!(observation(owner), before, "Render advanced Manual/First");
        assert_eq!(image.len(), (extent.0 * extent.1 * 4) as usize);
        image
    }
}

/// Release every loader/descendant gate BEFORE an Owner is synchronously
/// dropped on assertion unwind. Cancellation tests may temporarily move the
/// Owner to a separate caller thread; their gates remain protected here.
#[derive(Default)]
struct Releases {
    loaders: Vec<Option<futures::channel::oneshot::Sender<()>>>,
    descendants: Vec<Option<mpsc::Sender<()>>>,
}
impl Releases {
    fn loader(&mut self, index: usize) {
        if let Some(sender) = self.loaders[index].take() {
            let _ = sender.send(());
        }
    }
    fn descendant(&mut self, index: usize) {
        if let Some(sender) = self.descendants[index].take() {
            let _ = sender.send(());
        }
    }
    fn all(&mut self) {
        for index in 0..self.loaders.len() {
            self.loader(index);
        }
        for index in 0..self.descendants.len() {
            self.descendant(index);
        }
    }
}
impl Drop for Releases {
    fn drop(&mut self) {
        self.all();
    }
}
#[derive(Default)]
struct Retained {
    owner: Option<Owner>,
    releases: Releases,
}
impl Deref for Retained {
    type Target = Owner;
    fn deref(&self) -> &Owner {
        self.owner.as_ref().unwrap()
    }
}
impl DerefMut for Retained {
    fn deref_mut(&mut self) -> &mut Owner {
        self.owner.as_mut().unwrap()
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        self.releases.all();
        self.owner.take();
    }
}
fn wait(label: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < deadline, "Timed out: {label}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[derive(Default)]
struct Frames(u32);
impl Frames {
    fn step(&mut self, owner: &mut Owner, delta: f32, pressed: u32) {
        self.0 = self.0.wrapping_add(1);
        owner
            .poll(
                Ticks::from_f32(delta),
                true,
                Buttons {
                    pressed,
                    ..Buttons::default()
                },
                &PointerFrame {
                    number: self.0,
                    time_seconds: self.0 as f64 / 60.,
                    events: &[],
                },
            )
            .unwrap();
    }
}
fn observation(owner: &Owner) -> ([u32; 15], Vec<(Kind, bool)>, Vec<u32>) {
    let m = &owner.model;
    (
        [
            m.ready.into(),
            m.count as u32,
            m.available_count as u32,
            m.selected as u32,
            m.progress.to_bits(),
            m.scroll.value().to_bits(),
            m.close_flash.value().to_bits(),
            m.contact as u32,
            m.swipe.into(),
            m.closing.into(),
            m.complete.into(),
            owner.loader.completed.into(),
            owner.state().ready.into(),
            m.pending.len() as u32,
            m.actions.len() as u32,
        ],
        owner.scope.iter().map(|t| (t.kind, t.done)).collect(),
        owner.requests.clone(),
    )
}
fn first_pending(owner: &Owner, published: bool) {
    assert_eq!(owner.state().ready, published);
    assert!(!owner.loader.completed);
    assert!(!owner.model.ready, "Getter must not publish page discovery");
    assert_eq!(owner.model.available_count, 0);
    assert_eq!(owner.model.count, 0);
    assert_eq!(owner.model.selected, 0);
    assert_eq!(owner.model.progress.to_bits(), 0f32.to_bits());
    assert!(!owner.complete());
    assert!(owner.requests.is_empty());
    assert!(owner.model.actions.is_empty());
    assert!(owner.model.pending.is_empty());
    assert_eq!(
        owner
            .scope
            .iter()
            .map(|t| (t.kind, t.done))
            .collect::<Vec<_>>(),
        [(Kind::First, false)]
    );
}

#[test]
#[ignore = "real Manual atlas publication between update/render, completion-only18-page discovery, First/Open/Input/close callbacks,720p1080p GPU transport; exact Loopback"]
fn higurashi_manual_publication_before_completion_and_retained_first_prune() {
    let fixture = Fixture::new();
    let (before_tx, before_rx) = mpsc::channel();
    let (after_tx, after_rx) = mpsc::channel();
    let (release_before, held_before) = futures::channel::oneshot::channel();
    let (release_after, held_after) = futures::channel::oneshot::channel();
    let mut retained = Retained {
        owner: None,
        releases: Releases {
            loaders: vec![Some(release_before), Some(release_after)],
            descendants: Vec::new(),
        },
    };
    retained.owner = Some(
        Owner::construct(
            fixture.environment(),
            (1280, 720),
            Gates {
                before_publication: Some(Box::new(move |atlas| {
                    Box::pin(async move {
                        before_tx.send(atlas).unwrap();
                        let _ = held_before.await;
                    })
                })),
                after_publication: Some(Box::new(move |atlas| {
                    Box::pin(async move {
                        after_tx.send(atlas).unwrap();
                        let _ = held_after.await;
                    })
                })),
            },
        )
        .unwrap(),
    );
    let weak = before_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let publication = Arc::downgrade(&retained.loader.published);
    let retirement = retained.loader.job.as_ref().unwrap().retirement.clone();
    let mut frames = Frames::default();
    frames.step(&mut retained, 3., 1 << 25);
    first_pending(&retained, false);
    assert!(
        weak.upgrade().is_some(),
        "Actual archive was decoded before the gate"
    );
    assert!(super::super::view::packets(&retained.state(), false).is_empty());
    // No Owner.poll between releasing decoded resources and rendering below.
    retained.releases.loader(0);
    let published = after_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(Weak::ptr_eq(&weak, &published));
    first_pending(&retained, true);
    assert!(!retirement.is_finished());
    assert!(!super::super::view::packets(&retained.state(), false).is_empty());
    for extent in [(1280, 720), (1920, 1080)] {
        fixture.image(&retained, extent);
        first_pending(&retained, true);
    }
    frames.step(&mut retained, 9., 1 << 25);
    first_pending(&retained, true);
    // Successful worker output can finish without any main observation.
    retained.releases.loader(1);
    wait("Manual worker output completed unobserved", || {
        retirement.is_finished()
    });
    first_pending(&retained, true);
    assert!(retained.loader.job.as_ref().unwrap().task.is_some());
    // Scope retirement precedes publication of async_task's completed result
    // by a possible scheduling interval. Only actual Owner observation admits
    // First; every pending zero-delta poll must preserve the unpublished model.
    wait("Manual owner observed the real completed result", || {
        frames.step(&mut retained, 0., 0);
        if !retained.loader.completed {
            first_pending(&retained, true);
        }
        retained.loader.completed
    });
    assert!(retained.loader.completed && retained.model.ready);
    assert_eq!(retained.model.available_count, 18);
    assert_eq!(retained.state().count, 18);
    assert_eq!(retained.state().progress.to_bits(), 0f32.to_bits());
    assert_eq!(
        retained
            .scope
            .iter()
            .map(|t| (t.kind, t.done))
            .collect::<Vec<_>>(),
        [(Kind::First, true), (Kind::Open, false)]
    );
    assert!(retained.loader.job.is_some());
    frames.step(&mut retained, 5., 0);
    assert!(
        retained.loader.job.is_none(),
        "First prunes on the following snapshot"
    );
    assert!(
        weak.upgrade().is_some(),
        "First prune must retain the parent atlas"
    );
    assert_eq!(retained.state().progress.to_bits(), 0.5f32.to_bits());
    frames.step(&mut retained, 5., 0);
    assert_eq!(retained.state().progress.to_bits(), 1f32.to_bits());
    assert_eq!(
        retained
            .scope
            .iter()
            .map(|t| (t.kind, t.done))
            .collect::<Vec<_>>(),
        [(Kind::Open, true), (Kind::Input, false)]
    );
    frames.step(&mut retained, 0., 0);
    assert_eq!(retained.scope.len(), 1);
    assert!(retained.reference_state().2);
    assert!(retained.requests.is_empty());
    for extent in [(1280, 720), (1920, 1080)] {
        let image = fixture.image(&retained, extent);
        assert!(image.chunks_exact(4).any(|rgba| rgba[..3] != [0, 0, 0]));
    }
    frames.step(&mut retained, 0., 1 << 25);
    assert!(retained.model.closing);
    assert!(retained.requests.is_empty());
    let deadline = Instant::now() + Duration::from_secs(5);
    while retained.requests.is_empty() {
        frames.step(&mut retained, 3., 0);
        assert!(
            Instant::now() < deadline,
            "Manual close flash did not finish"
        );
    }
    assert_eq!(retained.take_requests(), [0]);
    assert_eq!(retained.state().progress.to_bits(), 1f32.to_bits());
    assert!(!retained.complete());
    frames.step(&mut retained, 5., 0);
    assert_eq!(retained.state().progress.to_bits(), 0.5f32.to_bits());
    assert!(retained.requests.is_empty());
    frames.step(&mut retained, 5., 0);
    assert!(retained.complete());
    assert_eq!(retained.state().progress.to_bits(), 0f32.to_bits());
    assert_eq!(retained.take_requests(), [1]);
    frames.step(&mut retained, 0., 0);
    assert!(retained.scope.is_empty());
    assert!(weak.upgrade().is_some());
    drop(retained);
    assert!(retirement.is_finished());
    assert!(weak.upgrade().is_none());
    assert!(publication.upgrade().is_none());
    assert_eq!(fixture.writes.0.load(Ordering::SeqCst), 0);
    eprintln!(
        "Manual: real update→early atlas→4 pure720p1080p transport frames, no early18-page discovery/Open/callbacks, actual First prune retains atlas, next-snapshot Open/Input, B close stages0/1, zero storage writes; no new independent pixel/native concurrency claim"
    );
}

struct NestedCleanup {
    entered: mpsc::Sender<bool>,
    release: mpsc::Receiver<()>,
    finished: mpsc::Sender<bool>,
}
struct TailDrop {
    atlas: Weak<CharacterTextures>,
    dropped: mpsc::Sender<bool>,
    nested: Option<NestedCleanup>,
}
impl Drop for TailDrop {
    fn drop(&mut self) {
        let _ = self.dropped.send(self.atlas.upgrade().is_some());
        if let Some(nested) = self.nested.take() {
            let atlas = self.atlas.clone();
            // Spawn while ScopedFuture destruction has re-entered the actual
            // loader scope. Discarding its receiver does not cancel the producer.
            drop(shin_tasks::compute::spawn(move || {
                let _ = nested.entered.send(atlas.upgrade().is_some());
                let _ = nested.release.recv();
                let _ = nested.finished.send(atlas.upgrade().is_some());
            }));
        }
    }
}
#[test]
#[ignore = "real Manual pre/post publication and completed-unobserved cancellation, nested scoped cleanup and weak resource lifetimes; exact Loopback, no window/user saves"]
fn higurashi_manual_publication_cancellation_drains_before_parent_atlas_release() {
    let fixture = Fixture::new();
    for (published, completed_unobserved) in [(false, false), (true, false), (true, true)] {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (tail_tx, tail_rx) = mpsc::channel();
        let (nested_tx, nested_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let (release_loader, held_loader) = futures::channel::oneshot::channel();
        let (release_nested, held_nested) = mpsc::channel();
        let mut retained = Retained {
            owner: None,
            releases: Releases {
                loaders: vec![Some(release_loader)],
                descendants: vec![Some(release_nested)],
            },
        };
        let gate: ReferenceGate = Box::new(move |atlas| {
            // Capture the Drop guard before constructing the pending future,
            // so cancellation cannot pass merely by never entering its body.
            let tail = TailDrop {
                atlas: atlas.clone(),
                dropped: tail_tx,
                nested: (!completed_unobserved).then_some(NestedCleanup {
                    entered: nested_tx,
                    release: held_nested,
                    finished: finished_tx,
                }),
            };
            Box::pin(async move {
                let _tail = tail;
                ready_tx.send(atlas).unwrap();
                let _ = held_loader.await;
            })
        });
        let mut gates = Gates::default();
        if published {
            gates.after_publication = Some(gate);
        } else {
            gates.before_publication = Some(gate);
        }
        retained.owner = Some(Owner::construct(fixture.environment(), (1280, 720), gates).unwrap());
        let weak = ready_rx.recv_timeout(Duration::from_secs(20)).unwrap();
        let publication = Arc::downgrade(&retained.loader.published);
        let retirement = retained.loader.job.as_ref().unwrap().retirement.clone();
        let mut frames = Frames::default();
        frames.step(&mut retained, 6., 1 << 25);
        first_pending(&retained, published);
        if completed_unobserved {
            retained.releases.loader(0);
            wait("Manual completed but unobserved cancellation", || {
                retirement.is_finished()
            });
            assert!(tail_rx.recv_timeout(Duration::from_secs(1)).unwrap());
            first_pending(&retained, true);
            drop(retained);
            assert!(retirement.is_finished());
        } else {
            let owner = retained.owner.take().unwrap();
            let (dropped_tx, dropped_rx) = mpsc::channel();
            let caller = std::thread::spawn(move || {
                drop(owner);
                let _ = dropped_tx.send(());
            });
            let during_nested = nested_rx.recv_timeout(Duration::from_secs(20)).unwrap();
            if published {
                assert!(tail_rx.recv_timeout(Duration::from_secs(1)).unwrap());
                assert!(
                    during_nested,
                    "Parent atlas must survive cancel/join and nested drain"
                );
                assert!(weak.upgrade().is_some());
            } else {
                // No parent archive exists. Cancellation drops the decoded
                // unpublished output; its temporary field destruction order is
                // not asserted as original scheduler interruption behavior.
                let _ = tail_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            }
            assert!(
                publication.upgrade().is_some(),
                "Publication storage died before drain"
            );
            assert!(!retirement.is_finished());
            assert!(matches!(
                dropped_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            retained.releases.descendant(0);
            let before_release = finished_rx.recv_timeout(Duration::from_secs(20)).unwrap();
            if published {
                assert!(before_release, "Published atlas died during nested drain");
            }
            dropped_rx.recv_timeout(Duration::from_secs(20)).unwrap();
            caller.join().unwrap();
            assert!(retirement.is_finished());
            drop(retained);
        }
        assert!(
            weak.upgrade().is_none(),
            "Manual atlas escaped owner retirement"
        );
        assert!(
            publication.upgrade().is_none(),
            "Manual publication storage escaped retirement"
        );
        assert_eq!(fixture.writes.0.load(Ordering::SeqCst), 0);
    }
    eprintln!(
        "Manual:3 real decoded cancellation schedules (unpublished,published-pending,completed-unobserved),2 cancellation-triggered nested scope drains, parent publication/atlas kept through join, weak resources released, no early Open/callback/write; original concurrent cancellation interleavings remain bounded"
    );
}
