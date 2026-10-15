//! Deterministic pending PIC cancellation across the real saved-scene factory.
use super::*;
use crate::{
    app::SceneHost,
    asset::{picture::Picture, system::AssetServer},
};
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires copied progress, supplied ROM, Metal and exact Loopback Audio"]
fn reference_live_picture_survives_retiring_producer() {
    assert!(std::env::var_os("SHIN_REFERENCE_SHARED_PICTURE_HANDOFF_ONLY").is_some());
    std::thread::Builder::new()
        .name("shared-picture-handoff-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(super::save_menu_render_reference::reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}
// Occupy compute workers without changing production decoders. The first real
// Picture future can publish a pending cache entry, but cannot finish decoding.
// Drop releases every worker even when a reference assertion fails.
struct ComputeGate {
    releases: Vec<std::sync::mpsc::Sender<()>>,
    tasks: Vec<shin_tasks::ComputeTask<()>>,
}
impl ComputeGate {
    fn hold() -> Self {
        let mut gate = Self {
            releases: Vec::new(),
            tasks: Vec::new(),
        };
        let (ready, started) = std::sync::mpsc::channel();
        for _ in 0..rayon::current_num_threads() {
            let ready = ready.clone();
            let (release, wait) = std::sync::mpsc::channel();
            gate.releases.push(release);
            gate.tasks.push(shin_tasks::compute::spawn(move || {
                ready.send(()).unwrap();
                let _ = wait.recv();
            }));
        }
        for _ in 0..gate.tasks.len() {
            started.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        gate
    }
}
impl Drop for ComputeGate {
    fn drop(&mut self) {
        for release in self.releases.drain(..) {
            let _ = release.send(());
        }
        for task in self.tasks.drain(..) {
            shin_tasks::block_on(task);
        }
    }
}
fn submit(ctx: &mut PreRenderContext) {
    let encoder = std::mem::replace(
        ctx.encoder,
        ctx.device.create_command_encoder(&Default::default()),
    );
    let mut upload = ctx.device.create_command_encoder(&Default::default());
    ctx.dynamic_buffer.finish(&mut upload);
    ctx.queue.submit([upload.finish(), encoder.finish()]);
    ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
}
pub(super) fn verify<'a>(ctx: &mut PreRenderContext<'a, '_, '_, '_>, server: &'a Arc<AssetServer>) {
    let source = std::env::var("SHIN_REFERENCE_SHARED_PICTURE_HANDOFF_SAVE").unwrap();
    assert!(Path::new(&source).starts_with(std::env::temp_dir()));
    let bytes = std::fs::read(&source).unwrap();
    let globals = SaveData::decode(&bytes).unwrap();
    let game = globals.manual_save_slots[4].as_ref().unwrap().clone();
    assert_eq!((game.entry.scenario_id, game.entry.save_position), (0, 119));
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let scene = reconstruction::ReconstructedScene::from_game_data(
        &assets.scenario,
        &game,
        &globals,
        1_000_000,
    )
    .unwrap();
    let (_, first_layer) = scene
        .state
        .layers
        .layerbanks
        .iter()
        .find(|(_, layer)| layer.layer_type.is_some())
        .unwrap();
    assert_eq!(
        first_layer.layer_type,
        Some(shin_core::vm::command::types::LayerType::Picture)
    );
    let path = assets.scenario.info_tables().picture_info[first_layer.params.0 as usize].path();
    eprintln!("Controlled saved-scene first Picture: {path}");
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let initial = Adv::new(
        manager.clone(),
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    let mut host = SceneHost::new(initial, assets, server.clone());
    let gate = ComputeGate::hold();
    // Same public Picture loader used by SaveMenuPreviewScene; cancellation
    // retires its inline cache producer when the preview task is discarded.
    let mut preview = Box::pin(server.load::<Picture, _>(&path));
    shin_tasks::block_on(async {
        assert!(futures::poll!(&mut preview).is_pending());
    });
    assert_eq!(
        server.context().picture_cache.reference_ownership_counts(),
        (0, 1)
    );
    host.reference_enter_saved_scene(
        GlobalData::open(None, Some(globals)).unwrap(),
        crate::audio::GlobalAudio::new(manager),
        game,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while server
        .reference_requested_paths()
        .iter()
        .filter(|p| p.as_str() == path.as_str())
        .count()
        < 2
    {
        assert!(
            Instant::now() < deadline,
            "Saved-scene factory never requested the shared authored picture"
        );
        std::thread::yield_now();
    }
    // The one I/O executor polls the scene before this later probe. Its Picture
    // await must have joined the pending cache generation before cancellation.
    shin_tasks::block_on(shin_tasks::async_io::spawn(async { 42 }));
    assert!(host.reference_adv().unwrap().is_loading_saved_scene());
    assert_eq!(
        server.context().picture_cache.reference_ownership_counts(),
        (0, 1)
    );
    drop(preview);
    drop(gate);
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    while host.reference_adv().unwrap().is_loading_saved_scene() {
        frame.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(30.0),
                asset_server: server,
                pre_render: ctx,
            },
            Default::default(),
            glam::Vec2::ZERO,
            glam::Vec2::ZERO,
            (1920, 1080),
            0.0,
            0.0,
        )
        .unwrap();
        submit(ctx);
        assert!(
            Instant::now() < deadline,
            "Live scene failed to settle after producer retirement"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let adv = host.reference_adv().unwrap();
    assert!(
        adv.saved_scene_load_error().is_none(),
        "Live saved scene inherited retired preview error: {:?}",
        adv.saved_scene_load_error()
    );
    assert_eq!(adv.reference_entry().save_position, 119);
    let picture = shin_tasks::block_on(server.load::<Picture, _>(&path)).unwrap();
    assert!(
        picture.blocks.len() > 0,
        "Actual decoded GPU blocks must be available"
    );
    assert!(
        server
            .context()
            .picture_cache
            .reference_ownership_counts()
            .1
            == 0
    );
    eprintln!(
        "Pending public Picture producer cancelled after real SceneHost saved-scene waiter joins: boundary119 published, GPU picture usable, no pending cache entry, no scene error"
    );
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}
