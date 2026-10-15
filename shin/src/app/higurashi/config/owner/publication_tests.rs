//! Actual decoded assets and scoped jobs resolve the publication interval that
//! the frozen native model transport does not execute concurrently. Parent runs
//! these serially on offscreen GPU/exact Loopback; no window or user save.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    app::higurashi::{
        first_boot::CommonAssets,
        load_game::{Source, Storage},
    },
    asset::system::AssetServer,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    sync::{
        Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(15);
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
        bail!("Unchanged Config must not write progress")
    }
}
fn environment(server: &Arc<AssetServer>, writes: &Arc<NoWrites>) -> Environment {
    let common = Arc::new(CommonAssets {
        scenario: server.load_sync("/main.snr").unwrap(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut sounds = crate::audio::SystemSoundPlayer::new(manager);
    sounds.bank = Some(common.sysse.clone());
    Environment {
        common,
        server: server.clone(),
        sounds: Arc::new(Mutex::new(sounds)),
        storage: Some(writes.clone()),
    }
}
fn until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !condition() {
        assert!(Instant::now() < deadline, "{label}");
        std::thread::yield_now();
    }
}
struct AsyncRelease(Arc<Mutex<Option<futures::channel::oneshot::Sender<()>>>>);
impl AsyncRelease {
    fn new() -> (Self, futures::channel::oneshot::Receiver<()>) {
        let (tx, rx) = futures::channel::oneshot::channel();
        (Self(Arc::new(Mutex::new(Some(tx)))), rx)
    }
    fn release(&self) {
        if let Some(tx) = self.0.lock().take() {
            let _ = tx.send(());
        }
    }
}
impl Drop for AsyncRelease {
    fn drop(&mut self) {
        self.release();
    }
}
struct BlockingRelease(Option<mpsc::Sender<()>>);
impl BlockingRelease {
    fn release(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}
impl Drop for BlockingRelease {
    fn drop(&mut self) {
        self.release();
    }
}
struct CancelSignal {
    weak: Weak<CharacterTextures>,
    tx: mpsc::Sender<bool>,
}
impl Drop for CancelSignal {
    fn drop(&mut self) {
        let _ = self.tx.send(self.weak.upgrade().is_some());
    }
}
fn poll(
    owner: &mut Owner,
    settings: &mut SaveData,
    number: &mut u32,
    delta: f32,
    pressed: u32,
) -> Result<()> {
    *number += 1;
    owner.poll(
        settings,
        None,
        &mut false,
        Ticks::from_f32(delta),
        true,
        Buttons {
            pressed,
            ..Buttons::default()
        },
        &PointerFrame {
            number: *number,
            time_seconds: f64::from(*number) / 60.,
            events: &[],
        },
    )
}
fn scope(owner: &Owner) -> Vec<(Kind, bool)> {
    owner
        .scope
        .iter()
        .map(|child| match child {
            Owned::Private(t) => (t.kind, t.done),
            _ => panic!("Unexpected Question/SaveGame"),
        })
        .collect()
}
fn pure_render(device: &wgpu::Device, queue: &wgpu::Queue, owner: &Owner, extent: (u32, u32)) {
    let before = owner.state();
    let tasks = scope(owner);
    let atlas = owner.loader.atlas().map(|a| Arc::as_ptr(&a) as usize);
    let pixels = reference_render_to_texture(device, queue, extent, |ctx, target| {
        owner.prepare(ctx);
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(ctx.depth_stencil),
            "Config early published atlas",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.),
        );
        owner.render(&mut pass).unwrap();
    });
    assert_eq!(pixels.len(), extent.0 as usize * extent.1 as usize * 4);
    // Native progress0 may put all parent art offscreen. Identity/readiness and
    // pure clock/scope state are the proof, never forced progress/nonblack art.
    let after = owner.state();
    assert_eq!(
        (
            before.ready,
            before.progress,
            before.scroll,
            before.frame,
            before.close_flash,
            before.reset_flash
        ),
        (
            after.ready,
            after.progress,
            after.scroll,
            after.frame,
            after.close_flash,
            after.reset_flash
        )
    );
    assert_eq!(scope(owner), tasks);
    assert_eq!(
        owner.loader.atlas().map(|a| Arc::as_ptr(&a) as usize),
        atlas
    );
}

#[test]
#[ignore = "actual Config atlas publication before completion,real First/Open/Input/unchanged B close,offscreen720p1080p/exact Loopback;no window/user saves"]
fn higurashi_config_owner_early_publication_keeps_first_completion_and_prune_order() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    for extent in [(1280, 720), (1920, 1080)] {
        let writes = Arc::new(NoWrites::default());
        let env = environment(&server, &writes);
        let mut settings = SaveData::default();
        settings.settings.common.v3_sysvol = 37;
        let baseline = serde_json::to_vec(&settings).unwrap();
        let (release, held) = AsyncRelease::new();
        let (published, publication) = mpsc::channel();
        let mut owner = Owner::with_gate(
            env,
            &settings,
            0,
            false,
            extent,
            Gate::Hold {
                at: PublicationPoint::After,
                callback: Some(Box::new(move |weak| {
                    Box::pin(async move {
                        published.send(weak).unwrap();
                        let _ = held.await;
                    })
                })),
            },
        )
        .unwrap();
        // Move the release guard AFTER Owner: assertion unwinding opens the
        // gate before mandatory synchronous Owner/Job retirement can wait.
        let release = release;
        let weak = publication
            .recv_timeout(DEADLINE)
            .expect("real Config publication");
        let id = Arc::as_ptr(&weak.upgrade().unwrap()) as usize;
        assert!(owner.state().ready);
        assert!(!owner.loader.completed);
        assert_eq!(scope(&owner), [(Kind::First, false)]);
        pure_render(&device, &queue, &owner, (extent.0 as u32, extent.1 as u32));
        assert_eq!(owner.model.progress, 0.);
        assert!(owner.take_requests().is_empty());
        let mut number = 0;
        poll(&mut owner, &mut settings, &mut number, 3., 1 << 25).unwrap();
        assert_eq!(owner.model.progress, 0.);
        assert_eq!(scope(&owner), [(Kind::First, false)]);
        assert!(!owner.reference_state().2);
        assert!(!owner.model.closing);
        assert_eq!(
            owner.scrollbar.visible, 770,
            "Published atlas admits clock tail while First waits"
        );
        release.release();
        let retirement = owner.loader.job.as_ref().unwrap().retirement.clone();
        until("Config worker finished independently", || {
            retirement.is_finished()
        });
        assert!(
            !owner.loader.completed,
            "No main poll has observed completion"
        );
        // Scoped work can retire just before async_task publishes its result.
        // Observe readiness through the actual Owner; pending delivery must
        // still leave First alone, with no Open/progress or return callback.
        until("Config owner observed the real completed result", || {
            poll(&mut owner, &mut settings, &mut number, 0., 0).unwrap();
            if !owner.loader.completed {
                assert_eq!(scope(&owner), [(Kind::First, false)]);
                assert_eq!(owner.model.progress, 0.);
                assert!(owner.requests.is_empty());
            }
            owner.loader.completed
        });
        assert_eq!(scope(&owner), [(Kind::First, true), (Kind::Open, false)]);
        assert_eq!(
            owner.model.progress, 0.,
            "New Open waits next borrowed snapshot"
        );
        let mut drawn_open = false;
        owner.scope.draw_ref(|child| {
            drawn_open |= matches!(
                child,
                Owned::Private(Task {
                    kind: Kind::Open,
                    ..
                })
            )
        });
        assert!(!drawn_open);
        poll(&mut owner, &mut settings, &mut number, 3., 0).unwrap();
        assert!(
            owner.loader.job.is_none(),
            "Normal First prune retires its job"
        );
        assert!(owner.model.progress > 0. && owner.model.progress < 1.);
        assert_eq!(Arc::as_ptr(&owner.loader.atlas().unwrap()) as usize, id);
        assert!(weak.upgrade().is_some(), "First prune retains parent atlas");
        for _ in 0..8 {
            poll(&mut owner, &mut settings, &mut number, 3., 0).unwrap();
            if scope(&owner) == [(Kind::Input, false)] {
                break;
            }
        }
        assert_eq!(scope(&owner), [(Kind::Input, false)]);
        assert_eq!(owner.model.progress, 1.);
        assert!(owner.reference_state().2);
        assert_eq!(serde_json::to_vec(&settings).unwrap(), baseline);
        assert!(owner.take_requests().is_empty());
        poll(&mut owner, &mut settings, &mut number, 0., 1 << 25).unwrap();
        assert!(owner.model.closing);
        let mut returns = owner.take_requests();
        for _ in 0..20 {
            poll(&mut owner, &mut settings, &mut number, 3., 0).unwrap();
            returns.extend(owner.take_requests());
            assert!(!owner.has_save_task());
            if owner.complete() {
                break;
            }
        }
        // Existing519a0 sends0 before538b8 eventually sends1. Only1 lets the
        // external caller retire/reopen its source; no new callback is added.
        assert_eq!(returns, [0, 1]);
        assert!(owner.complete());
        assert_eq!(owner.model.progress, 0.);
        assert_eq!(writes.0.load(Ordering::SeqCst), 0);
        assert_eq!(serde_json::to_vec(&settings).unwrap(), baseline);
        assert!(weak.upgrade().is_some());
        drop(owner);
        assert!(weak.upgrade().is_none());
        assert!(retirement.is_finished());
    }
    eprintln!(
        "Config owner:2 real atlas publication intervals,2 progress0 pure720p1080p frames,First completion/Open-next-snapshot/Input,atlas retained at First prune,unchanged B close existing[0,1],zero writes/complete settings unchanged;no native concurrency/window acceptance"
    );
}

#[test]
#[ignore = "actual Config worker publication between scope and clock/render,offscreen/exact Loopback fixture,no window/user saves"]
fn higurashi_config_owner_resamples_publication_after_scope_and_before_render() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let writes = Arc::new(NoWrites::default());
    let env = environment(&server, &writes);
    let mut settings = SaveData::default();
    let (release, held) = AsyncRelease::new();
    let (entered, entry) = mpsc::channel();
    let mut owner = Owner::with_gate(
        env,
        &settings,
        3,
        true,
        (1920, 1080),
        Gate::Hold {
            at: PublicationPoint::Before,
            callback: Some(Box::new(move |weak| {
                Box::pin(async move {
                    entered.send(weak).unwrap();
                    let _ = held.await;
                })
            })),
        },
    )
    .unwrap();
    let release = release;
    let weak = entry.recv_timeout(DEADLINE).unwrap();
    assert!(!owner.state().ready);
    let mut number = 0;
    poll(&mut owner, &mut settings, &mut number, 0., 0).unwrap();
    assert_eq!(owner.scrollbar.visible, 1);
    assert_eq!(scope(&owner), [(Kind::First, false)]);
    // Seed an independently verified native linear tween. First still has no
    // input/Open; delta3 must advance this scroll exactly1, not0 or3.
    owner
        .model
        .scroll
        .control(40., Ticks::from_f32(4.), 0)
        .unwrap();
    let published = owner.loader.published.clone();
    let resume = release.0.clone();
    owner.after_scope = Some(Box::new(move || {
        if let Some(tx) = resume.lock().take() {
            let _ = tx.send(());
        }
        until("Actual publication after First poll", || {
            published.lock().is_some()
        });
    }));
    poll(&mut owner, &mut settings, &mut number, 3., 0).unwrap();
    assert!(owner.state().ready);
    assert!(!owner.loader.completed);
    assert_eq!(scope(&owner), [(Kind::First, false)]);
    assert_eq!(owner.model.progress, 0.);
    assert_eq!(
        owner.scrollbar.position, 0,
        "Scrollbar precedes1tick update"
    );
    assert_eq!(owner.scrollbar.visible, 770);
    assert_eq!(owner.model.scroll.value(), 10.);
    assert!(owner.take_requests().is_empty());
    assert_eq!(writes.0.load(Ordering::SeqCst), 0);
    pure_render(&device, &queue, &owner, (1920, 1080));
    assert!(weak.upgrade().is_some());
    drop(owner);
    assert!(weak.upgrade().is_none());

    // A separate actual schedule publishes after the last complete Owner poll
    //and before its next render. No getter/main task polls the completed output.
    let env = environment(&server, &writes);
    let (release, held) = AsyncRelease::new();
    let (entered, entry) = mpsc::channel();
    let mut owner = Owner::with_gate(
        env,
        &settings,
        0,
        false,
        (1280, 720),
        Gate::Hold {
            at: PublicationPoint::Before,
            callback: Some(Box::new(move |weak| {
                Box::pin(async move {
                    entered.send(weak).unwrap();
                    let _ = held.await;
                })
            })),
        },
    )
    .unwrap();
    let release = release;
    let weak = entry.recv_timeout(DEADLINE).unwrap();
    poll(&mut owner, &mut settings, &mut number, 0., 0).unwrap();
    let frame = owner.model.frame;
    assert!(!owner.state().ready);
    release.release();
    until("Actual publication before next render", || {
        owner.loader.atlas().is_some()
    });
    assert!(owner.state().ready);
    assert_eq!(owner.model.frame, frame);
    assert!(!owner.loader.completed);
    pure_render(&device, &queue, &owner, (1280, 720));
    assert_eq!(owner.model.progress, 0.);
    assert!(owner.take_requests().is_empty());
    drop(owner);
    assert!(weak.upgrade().is_none());
    eprintln!(
        "Config owner:actual post-scope and update→render publication,pre-scroll scrollbar and exact1tick with delta3,read-only fresh atlas render,First still pending/no Open/progress/callback;2 offscreen frames,no native concurrency/window claim"
    );
}

#[test]
#[ignore = "actual Config loader before/after publication+completed-unobserved cancellation,nested descendant drain,real texture owners;offscreen GPU,no audio/window/save"]
fn higurashi_config_loader_cancellation_drains_before_clearing_parent_atlas() {
    let (_, _, server) = reference_gpu_assets();
    let server = Arc::new(server);
    for at in [PublicationPoint::Before, PublicationPoint::After] {
        for completed_unobserved in [false, true] {
            let (release, held) = AsyncRelease::new();
            let (entered, entry) = mpsc::channel();
            let loader = Loader::with_gate(
                server.clone(),
                Gate::Hold {
                    at,
                    callback: Some(Box::new(move |weak| {
                        Box::pin(async move {
                            entered.send(weak).unwrap();
                            let _ = held.await;
                        })
                    })),
                },
            );
            let release = release;
            let weak = entry.recv_timeout(DEADLINE).unwrap();
            let published = loader.published.clone();
            assert_eq!(published.lock().is_some(), at == PublicationPoint::After);
            let retirement = loader.job.as_ref().unwrap().retirement.clone();
            if completed_unobserved {
                release.release();
                until("Unobserved real Config result finished", || {
                    retirement.is_finished()
                });
                assert!(published.lock().is_some());
            }
            assert!(!loader.completed);
            drop(loader);
            assert!(retirement.is_finished());
            assert!(published.lock().is_none());
            assert!(weak.upgrade().is_none());
        }
    }
    // Execute actual loader cancellation on a caller thread while a registered
    //compute descendant remains held. It creates a further descendant AFTER
    //cancellation; Parent publication must survive both drains before clear.
    let (started, start) = mpsc::channel();
    let (first_release, first) = mpsc::channel();
    let (nested, nesting) = mpsc::channel();
    let (last_release, last) = mpsc::channel();
    let (cancelled, cancel) = mpsc::channel();
    let loader = Loader::with_gate(
        server,
        Gate::Hold {
            at: PublicationPoint::After,
            callback: Some(Box::new(move |weak| {
                Box::pin(async move {
                    let _cancel = CancelSignal {
                        weak: weak.clone(),
                        tx: cancelled,
                    };
                    shin_tasks::compute::spawn(move || {
                        started.send(weak.clone()).unwrap();
                        first.recv_timeout(DEADLINE).unwrap();
                        shin_tasks::compute::spawn_and_forget(move || {
                            nested.send(()).unwrap();
                            last.recv_timeout(DEADLINE).unwrap();
                            assert!(
                                weak.upgrade().is_some(),
                                "Parent atlas cleared before nested descendant retired"
                            );
                        });
                    })
                    .await;
                })
            })),
        },
    );
    // Declared after loader; unwind always unblocks descendants before any
    //caller could synchronously wait for their mandatory retirement.
    let mut release_first = BlockingRelease(Some(first_release));
    let mut release_last = BlockingRelease(Some(last_release));
    let weak = start.recv_timeout(DEADLINE).unwrap();
    let published = loader.published.clone();
    let retirement = loader.job.as_ref().unwrap().retirement.clone();
    let (done, finished) = mpsc::channel();
    let join = std::thread::spawn(move || {
        drop(loader);
        done.send(()).unwrap();
    });
    assert!(
        cancel.recv_timeout(DEADLINE).unwrap(),
        "Publication survives top-future cancellation"
    );
    assert!(!retirement.is_finished());
    assert!(finished.try_recv().is_err());
    assert!(published.lock().is_some());
    assert!(weak.upgrade().is_some());
    let mut unrelated = shin_tasks::async_io::spawn(async { 42 });
    assert_eq!(unrelated.poll_with_timeout(DEADLINE), Some(42));
    release_first.release();
    nesting.recv_timeout(DEADLINE).unwrap();
    assert!(!retirement.is_finished());
    assert!(finished.try_recv().is_err());
    assert!(weak.upgrade().is_some());
    release_last.release();
    finished.recv_timeout(DEADLINE).unwrap();
    join.join().unwrap();
    assert!(retirement.is_finished());
    assert!(published.lock().is_none());
    assert!(weak.upgrade().is_none());
    eprintln!(
        "Config loader:4 real before/after and completed-unobserved cancellations,actual top-future cancellation before held descendant drain,new nested descendant after cancellation,I/O executor stays available,parent atlas alive until all descendants retire,then slot/weak cleared;no native scheduler race/decoder/GPU-pixel/audio/window/save claim"
    );
}
