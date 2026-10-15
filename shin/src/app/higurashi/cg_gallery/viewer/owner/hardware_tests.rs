//! Real decoder/GPU transport and resource ownership checks. These exercise
//! the concrete backend; standalone captures do not claim the full CG parent.
use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::sync::atomic::AtomicUsize;

fn args(identity: u64, calls: Arc<AtomicUsize>) -> LoadArgs {
    LoadArgs {
        identity,
        seeds: Seeds::new(move || {
            let n = calls.fetch_add(1, Ordering::SeqCst) as u32;
            [11 + n, 23 + n, 37 + n, 53 + n]
        }),
    }
}
fn asset_path(scenario: &HigurashiScenario) -> String {
    let id = scenario.picture_box_info[0].picture_ids.0[0];
    let name = crate::layer::user::higurashi::selected_asset_name(
        scenario.picture_info[id as usize].name.as_str(),
        0,
    );
    format!("/picture/{name}.pic")
}
#[test]
#[ignore = "actual Higurashi assets/offscreen Metal; fresh owner entropy versus shared PIC blocks; no audio/window/save"]
fn higurashi_cg_uncached_picture_owners_keep_cached_asset_and_shared_blocks() {
    let (_, _, server) = reference_gpu_assets();
    let scenario: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let path = asset_path(&scenario);
    let calls = Arc::new(AtomicUsize::new(0));
    let args = args(71, calls.clone());
    let cached =
        shin_tasks::block_on(server.load_with_args::<PreparedPicture, _>(&path, args.clone()))
            .unwrap();
    let first = shin_tasks::block_on(
        server.load_uncached_with_args::<PreparedPicture, _>(&path, args.clone()),
    )
    .unwrap();
    let second = shin_tasks::block_on(
        server.load_uncached_with_args::<PreparedPicture, _>(&path, args.clone()),
    )
    .unwrap();
    let again =
        shin_tasks::block_on(server.load_with_args::<PreparedPicture, _>(&path, args)).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "one four-seed sample per fresh owner; cached lookup consumes none"
    );
    assert!(
        Arc::ptr_eq(&cached, &again),
        "uncached loads replaced or invalidated the ordinary cached owner"
    );
    assert!(!Arc::ptr_eq(&cached.art, &first.art));
    assert!(!Arc::ptr_eq(&first.art, &second.art));
    assert!(Arc::ptr_eq(&cached.art.blocks, &first.art.blocks));
    assert!(Arc::ptr_eq(&first.art.blocks, &second.art.blocks));
    assert!(
        first.art.blocks.len() > 0,
        "supplied CG fixture must contain real decoded/uploaded art"
    );
    assert_eq!(
        server
            .reference_requested_paths()
            .iter()
            .filter(|p| **p == path)
            .count(),
        4
    );
    println!(
        "CG uncached owner:3 fresh four-seed constructors,one retained ordinary cached owner,shared nonempty decoded PIC blocks; no audio/window/save"
    );
}

struct Initial;
impl Services for Initial {
    fn prepare_picture(&mut self, _: u16) -> Result<()> {
        Ok(())
    }
    fn worker_status(&mut self) -> u32 {
        0
    }
    fn take_prepared(&mut self) -> Result<Picture> {
        bail!("no initial worker")
    }
    fn capture_parent(&mut self, _: &Model) -> Result<()> {
        bail!("no initial capture")
    }
    fn publish_capture(&mut self, _: &Model) -> Result<()> {
        bail!("no initial publication")
    }
    fn input(&mut self, _: &mut Model, _: bool) -> Result<()> {
        bail!("no initial input")
    }
    fn cue(&mut self, _: Cue) -> Result<()> {
        Ok(())
    }
    fn disable_input(&mut self) {}
}
struct CaptureProbe {
    device: wgpu::Device,
    queue: wgpu::Queue,
    extent: (u32, u32),
    expected_identity: u64,
    live: Weak<PictureAsset>,
    previous: wgpu::TextureView,
    fail: bool,
    calls: usize,
}
impl Capture for CaptureProbe {
    fn capture_parent(&mut self, c: &Composition<'_>) -> Result<RenderTexture> {
        self.calls += 1;
        assert!(c.badges_hidden());
        assert_eq!(c.old_picture_identity(), Some(self.expected_identity));
        assert!(
            self.live.upgrade().is_some(),
            "old live art retired before capture"
        );
        assert_eq!(
            c.snapshot.unwrap().as_texture_source().view,
            &self.previous,
            "old capture replaced before composition callback"
        );
        ensure!(!self.fail, "injected capture failure");
        let mut captured = None;
        let pixels =
            reference_render_to_texture(&self.device, &self.queue, self.extent, |ctx, target| {
                let mut texture =
                    ctx.new_render_texture("CG standalone hidden-badge capture".into());
                {
                    let mut pass = ctx.begin_pass(
                        texture.as_texture_target(),
                        Some(ctx.depth_stencil),
                        "CG old live picture capture",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                    c.render(&mut pass).unwrap();
                }
                // A second getter draw of this immutable composition provides
                // nonempty-art readback. It is not a separate native pixel oracle.
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(ctx.depth_stencil),
                    "CG standalone art readback",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                c.render(&mut pass).unwrap();
                drop(pass);
                captured = Some(texture);
            });
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0),
            "capture omitted supplied live picture art"
        );
        Ok(captured.unwrap())
    }
    fn cue(&mut self, _: Cue) -> Result<()> {
        Ok(())
    }
}
fn model_picture(identity: u64, art: &PictureAsset, properties: HigurashiProperties) -> Picture {
    Picture {
        identity,
        metadata: Metadata {
            origin: [art.origin_x as i16, art.origin_y as i16],
            extent: [
                art.effective_width.try_into().unwrap(),
                art.effective_height.try_into().unwrap(),
            ],
        },
        properties,
    }
}
#[test]
#[ignore = "actual Higurashi PIC/atlas and offscreen Metal; concrete staged capture/lifetime/failure boundaries; no audio/window/save"]
fn higurashi_cg_viewer_stages_capture_until_old_art_retires_and_preserves_on_error() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let scenario: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let atlas = server.load_sync("/cgmode.txa").unwrap();
    let path = asset_path(&scenario);
    let calls = Arc::new(AtomicUsize::new(0));
    let seeds = args(1, calls.clone()).seeds;
    for (width, height) in [(1280, 720), (1920, 1080)] {
        let old = shin_tasks::block_on(
            server.load_uncached_with_args::<PreparedPicture, _>(&path, args(11, calls.clone())),
        )
        .unwrap();
        let next = shin_tasks::block_on(
            server.load_uncached_with_args::<PreparedPicture, _>(&path, args(12, calls.clone())),
        )
        .unwrap();
        let old_weak = Arc::downgrade(&old.art);
        let new_weak = Arc::downgrade(&next.art);
        let mut model = Model::new(
            super::super::super::Entry {
                name: "real CG backend boundary".into(),
                pictures: vec![0, 1],
                unlocked: vec![true, true],
            },
            &mut Initial,
        )
        .unwrap();
        model.picture = Some(model_picture(old.identity, &old.art, old.properties));
        model.apply_bounds(false);
        model.show_info = false;
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            width, height,
        )));
        let mut snapshot = RenderTexture::new(
            device.clone(),
            resize.handle(),
            "previous CG capture".into(),
        );
        snapshot.as_texture_target();
        let previous = snapshot.as_texture_source().view.clone();
        let mut capture = CaptureProbe {
            device: device.clone(),
            queue: queue.clone(),
            extent: (width, height),
            expected_identity: 11,
            live: old_weak.clone(),
            previous: previous.clone(),
            fail: true,
            calls: 0,
        };
        let mut backend = Backend {
            job: None,
            prepared: Arc::new(parking_lot::Mutex::new(Some(Ok(next)))),
            live: Some(Art {
                identity: old.identity,
                picture: old.art,
            }),
            snapshot: Some(snapshot),
            staged_snapshot: None,
            fault: None,
        };
        let mut core = Core {
            backend: &mut backend,
            server: &server,
            scenario: &scenario,
            seeds: &seeds,
            atlas: &atlas,
            capture: Some(&mut capture),
        };
        assert!(core.capture_parent(&model).is_err());
        assert!(core.backend.staged_snapshot.is_none());
        assert!(old_weak.upgrade().is_some());
        assert_eq!(
            core.backend
                .snapshot
                .as_ref()
                .unwrap()
                .as_texture_source()
                .view,
            &previous
        );
        drop(core);
        capture.fail = false;
        let mut core = Core {
            backend: &mut backend,
            server: &server,
            scenario: &scenario,
            seeds: &seeds,
            atlas: &atlas,
            capture: Some(&mut capture),
        };
        core.capture_parent(&model).unwrap();
        assert!(core.backend.staged_snapshot.is_some());
        assert!(old_weak.upgrade().is_some());
        assert_eq!(
            core.backend
                .snapshot
                .as_ref()
                .unwrap()
                .as_texture_source()
                .view,
            &previous
        );
        model.picture = Some(core.take_prepared().unwrap());
        model.apply_bounds(false);
        assert!(
            old_weak.upgrade().is_none(),
            "actual old Picture asset must retire before capture publication"
        );
        assert_eq!(
            core.backend
                .snapshot
                .as_ref()
                .unwrap()
                .as_texture_source()
                .view,
            &previous
        );
        core.publish_capture(&model).unwrap();
        assert!(core.backend.staged_snapshot.is_none());
        let replacement = core
            .backend
            .snapshot
            .as_ref()
            .unwrap()
            .as_texture_source()
            .view
            .clone();
        assert_ne!(replacement, previous);
        drop(core);
        capture.expected_identity = 12;
        capture.live = new_weak.clone();
        capture.previous = replacement.clone();
        model.closing = true;
        model.show_info = false;
        let mut core = Core {
            backend: &mut backend,
            server: &server,
            scenario: &scenario,
            seeds: &seeds,
            atlas: &atlas,
            capture: Some(&mut capture),
        };
        core.capture_parent(&model).unwrap();
        assert!(new_weak.upgrade().is_some());
        assert_eq!(
            core.backend
                .snapshot
                .as_ref()
                .unwrap()
                .as_texture_source()
                .view,
            &replacement
        );
        model.picture = None;
        core.publish_capture(&model).unwrap();
        assert!(new_weak.upgrade().is_none());
        assert!(core.backend.live.is_none());
        assert_ne!(
            core.backend
                .snapshot
                .as_ref()
                .unwrap()
                .as_texture_source()
                .view,
            &replacement
        );
        drop(core);
        assert_eq!(capture.calls, 3);
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        4,
        "capture/failure/publication must not reconstruct or resample Pictures"
    );
    println!(
        "CG concrete backend:two real Ready replacements/two close captures/two capture failures,both720p1080p,old picture alive through hidden-badge composition and retired before staged publication,nonempty actual art,zero capture-time seed samples; standalone transport,no audio/window/save"
    );
}
