use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Events(Vec<Action>);
impl Services for Events {
    fn capture_parent(&mut self, _: &Composition<'_, '_>) -> Result<RenderTexture> {
        bail!("publication check does not open a Viewer")
    }
    fn event(&mut self, a: Action) -> Result<()> {
        self.0.push(a);
        Ok(())
    }
}
#[test]
#[ignore = "actual scoped CG loader,atlas/font/offscreen Metal; publication before completion and cancellation; no audio/window/save"]
fn higurashi_cg_atlas_renders_before_mask_completion_and_pending_cancel_joins() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let scenario: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let settings = SaveData::default();
    for cancel in [false, true] {
        let (published_tx, published_rx) = mpsc::channel();
        let (release_tx, release_rx) = futures::channel::oneshot::channel();
        let mut events = Events::default();
        let seeds = Arc::new(AtomicUsize::new(0));
        let count = seeds.clone();
        let mut owner = Owner::construct(
            server.clone(),
            scenario.clone(),
            font.clone(),
            &settings,
            &mut None,
            Seeds::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                [1, 2, 3, 4]
            }),
            (1280, 720),
            &mut events,
            Gate::AfterAtlas(Box::new(move |atlas| {
                Box::pin(async move {
                    published_tx.send(atlas).unwrap();
                    let _ = release_rx.await;
                })
            })),
        )
        .unwrap();
        let atlas = published_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("actual atlas worker publication");
        assert!(atlas.upgrade().is_some());
        assert!(
            !owner.model.as_ref().unwrap().ready,
            "no main poll has published model state"
        );
        assert!(owner.loader.mask.is_none() && owner.loader.published.lock().mask.is_none());
        assert!(
            owner
                .loader
                .job
                .as_mut()
                .unwrap()
                .task
                .as_mut()
                .unwrap()
                .poll_naive()
                .is_none()
        );
        let state = format!("{:?}", owner.state());
        for extent in [(1280, 720), (1920, 1080)] {
            let pixels = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(ctx.depth_stencil),
                    "CG atomic atlas before First completion",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.),
                );
                owner.render(&mut pass).unwrap();
            });
            assert!(
                pixels
                    .chunks_exact(4)
                    .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0),
                "parent rendering missed worker-published atlas before main poll"
            );
        }
        assert_eq!(
            format!("{:?}", owner.state()),
            state,
            "early atlas draw changed model clocks/ready state"
        );
        assert_eq!(
            seeds.load(Ordering::SeqCst),
            0,
            "grid construction/render must not sample Viewer seeds"
        );
        if cancel {
            drop(owner);
            assert!(
                release_tx.send(()).is_err(),
                "drop must cancel/join the held worker"
            );
            assert!(
                atlas.upgrade().is_none(),
                "parent/held-worker atlas escaped retirement"
            );
        } else {
            release_tx.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut ready = false;
            while Instant::now() < deadline {
                owner
                    .poll(
                        Ticks::from_f32(1.),
                        false,
                        Input::default(),
                        &PointerFrame {
                            number: 0,
                            time_seconds: 0.,
                            events: &[],
                        },
                        &mut events,
                    )
                    .unwrap();
                ready = owner.scope.iter().any(|x| {
                    matches!(
                        x,
                        Owned::Private(Task {
                            kind: Kind::Input,
                            ..
                        })
                    )
                });
                if ready {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                ready,
                "completed real First did not publish actual Input task"
            );
            assert!(events.0.iter().any(|a| matches!(
                a,
                Action::PlayBgm {
                    id: 75,
                    loops: 0,
                    fade: 60
                }
            )));
            drop(owner);
            assert!(atlas.upgrade().is_none());
        }
    }
    println!(
        "CG actual First:worker-published atlas rendered at720p/1080p before mask/status2,render pure and Input still gated,completed opening and held cancellation/join retire atlas,zero Viewer seeds; no audio/window/save"
    );
}
