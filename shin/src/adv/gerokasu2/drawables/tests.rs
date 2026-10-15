use super::*;
use crate::{
    adv::gerokasu2::{
        layer_load::LayerLoadController,
        message_commands::MessageCommandContext,
        runner::{Gerokasu2Runner, InitialMessage, RunnerServices, RunnerStatus},
        voice::MessageNotifications,
    },
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, LayeredAssetIo},
    },
    layer::{Layer, render_params::TransformParams},
};
use binrw::BinWrite;
use shin_audio::AudioManager;
use shin_core::{
    format::{rom::RomReader, scenario::instruction_elements::NumberSpec},
    primitives::stateless_reader::StatelessFile,
    time::Ticks,
    vm::{
        command::types::{LayerId, LayerProperty},
        gerokasu2::message_face::PortraitChange,
    },
};
use shin_render::PassKind;
use std::io::{Cursor, Read};

fn bytes() -> Vec<u8> {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    data
}
fn command(kind: i32, flags: i32, params: [i32; 8]) -> Gerokasu2RuntimeCommand {
    Gerokasu2RuntimeCommand::LayerLoad {
        layer_id: 3,
        layer_type: kind,
        flags,
        params: (
            params[0], params[1], params[2], params[3], params[4], params[5], params[6], params[7],
        ),
    }
}
fn load(
    backend: &mut BasicLoadBackend,
    layers: &mut Gerokasu2Layers,
    cmd: Gerokasu2RuntimeCommand,
) -> LayerLoadController {
    let plan = backend.prepare_plan(layers, &cmd).unwrap();
    LayerLoadController::new(plan)
}
fn complete(controller: &mut LayerLoadController, backend: &mut BasicLoadBackend) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !controller.poll(backend).unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn retained_basic_load_owners_reuse_keep_reset_error_retry_and_runner() {
    let gpu = Offscreen::new();
    let mut parsed = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let missing_mask_id = parsed.mask_info.len() as i32;
    parsed
        .mask_info
        .push(shin_core::format::scenario::gerokasu2::MaskInfoItem {
            name: shin_core::format::text::utf8::U16Utf8String::new("欠落_移行マスク😀"),
        });
    let scenario = Arc::new(parsed);
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    struct TestFiles(std::path::PathBuf);
    impl Drop for TestFiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let overlay = TestFiles(
        std::env::temp_dir().join(format!("shin-gerokasu2-load-repair-{}", std::process::id())),
    );
    std::fs::create_dir_all(&overlay.0).unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&overlay.0).unwrap();
    io.try_with_rom(&path).unwrap();
    let assets = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let mut backend = BasicLoadBackend::new(
        assets.clone(),
        scenario.clone(),
        LoadCounters::new(0, [0; 48]),
    );
    let mut layers = Gerokasu2Layers::default();
    let tile = [0xf123, -960, -540, 1920, 1080, 0, 0, 0];
    let mut controller = load(&mut backend, &mut layers, command(1, 0, tile));
    complete(&mut controller, &mut backend);
    let bank = layers.target(LayerId::new(3)).unwrap().layerbank;
    let plane = PlaneId::new(0);
    assert!(!backend.has_back(), "ordinary load edits the active page");
    backend
        .layer_mut(plane, bank)
        .unwrap()
        .layer
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(137.0);
    let mut same = load(&mut backend, &mut layers, command(1, 2, tile));
    assert!(!same.plan().needs_loader);
    complete(&mut same, &mut backend);
    assert_eq!(
        backend
            .layer(plane, bank)
            .unwrap()
            .layer
            .properties()
            .get_value(LayerProperty::TranslateX),
        137.0
    );
    let mut reset = load(&mut backend, &mut layers, command(1, 0, tile));
    assert!(!reset.plan().needs_loader);
    complete(&mut reset, &mut backend);
    assert_eq!(
        backend
            .layer(plane, bank)
            .unwrap()
            .layer
            .properties()
            .get_value(LayerProperty::TranslateX),
        0.0
    );
    let mut picture = load(&mut backend, &mut layers, command(2, 2, [0; 8]));
    complete(&mut picture, &mut backend);
    let mut missing = load(
        &mut backend,
        &mut layers,
        command(3, 0, [0, 1, 0, 0, 0, 0, 0, 0]),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match missing.poll(&mut backend) {
            Err(_) => break,
            Ok(false) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(true) => panic!("Missing L bustup must not publish"),
        }
    }
    assert!(backend.resource_error().is_some());
    assert!(matches!(
        backend.layer(plane, bank).unwrap().layer,
        crate::adv::gerokasu2::layer_factory::GerDrawable::Shared(
            crate::layer::user::UserLayer::Picture(_)
        )
    ));
    // Supply the exact requested L path in a temporary overlay. The unchanged
    // ROM provides the fixture bytes; neither its data nor the pending request
    // is edited, and the backend does not substitute M for the missing L path.
    let name = scenario.bustup_info[0].name.as_str();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let source = rom.find_file(&format!("/bustup/M/{name}.bup")).unwrap();
    let mut data = Vec::new();
    rom.open_file(source)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    let replacement = overlay.0.join(format!("bustup/L/{name}.bup"));
    std::fs::create_dir_all(replacement.parent().unwrap()).unwrap();
    std::fs::write(replacement, data).unwrap();
    backend.retry_resource(missing.plan()).unwrap();
    complete(&mut missing, &mut backend);
    assert!(matches!(
        backend.layer(plane, bank).unwrap().layer,
        crate::adv::gerokasu2::layer_factory::GerDrawable::Shared(
            crate::layer::user::UserLayer::Bustup(_)
        )
    ));
    let mut gated = load(&mut backend, &mut layers, command(1, 4, tile));
    backend.set_transition(LoadTransition {
        kind: 1,
        duration: 60,
        params: [missing_mask_id, 500, 0, 0, 0, 0, 0, 0],
        ..Default::default()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match gated.poll(&mut backend) {
            Err(_) => break,
            Ok(false) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(true) => panic!("Missing transition mask must not publish"),
        }
    }
    assert!(backend.transition_error().is_some());
    assert!(matches!(
        backend.layer(plane, bank).unwrap().layer,
        crate::adv::gerokasu2::layer_factory::GerDrawable::Shared(
            crate::layer::user::UserLayer::Bustup(_)
        )
    ));
    // Skip cancels only the failed optional transition; the prepared Tile is
    // published through the same retained command without changing its request.
    backend.set_frame(Instant::now(), true);
    complete(&mut gated, &mut backend);
    backend.set_frame(Instant::now(), false);
    backend.set_transition(Default::default());
    let pixels = crate::adv::gerokasu2::message_ui::reference_frame(
        &gpu.device,
        &gpu.queue,
        (640, 360),
        false,
        |pass| {
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                backend.layer(plane, bank).unwrap().layer.render(
                    pass,
                    &TransformParams::default(),
                    3,
                    kind,
                );
            }
        },
    );
    assert!(pixels.chunks_exact(4).any(|p| p != [96, 136, 184, 255]));

    struct Services {
        loader: BasicLoadBackend,
    }
    impl MessageNotifications for Services {
        fn observed_voice(&mut self, _: &str) {}
        fn completed(&mut self, _: u32) {}
    }
    impl RunnerServices for Services {
        fn layer_load_services(&mut self) -> Option<&mut (dyn LoadServices + '_)> {
            Some(&mut self.loader)
        }
        fn command_context(
            &mut self,
            _: &Gerokasu2RuntimeCommand,
            _: bool,
        ) -> Result<MessageCommandContext> {
            self.loader.set_frame(Instant::now(), false);
            Ok(MessageCommandContext {
                was_read: false,
                event_mode: -1,
                skip: false,
                message_speed: 50,
            })
        }
        fn message_installed(&mut self, _: u32, _: PortraitChange) -> Result<()> {
            Ok(())
        }
        fn portrait_changed(&mut self, _: PortraitChange) -> Result<()> {
            Ok(())
        }
        fn advance_layers(&mut self, _: &Gerokasu2Layers, _: Ticks, _: bool) -> Result<()> {
            Ok(())
        }
        fn fast_forward_layers(&mut self, _: &Gerokasu2Layers) -> Result<()> {
            Ok(())
        }
    }
    let mut code = bytes();
    let offset = scenario.header.code_offset as usize;
    code.truncate(offset);
    let number = |out: &mut Vec<u8>, v: i32| {
        let mut cursor = Cursor::new(Vec::new());
        NumberSpec::<i32>::constant(v)
            .write_le(&mut cursor)
            .unwrap();
        out.extend(cursor.into_inner());
    };
    code.push(0xc1);
    for v in [3, 1, 0] {
        number(&mut code, v);
    }
    code.push(0x1f);
    for v in tile[..5].iter().copied() {
        number(&mut code, v);
    }
    code.extend([0, 0]);
    number(&mut code, 0);
    let size = code.len() as u32;
    code[4..8].copy_from_slice(&size.to_le_bytes());
    let synthetic = Arc::new(Gerokasu2Scenario::new(code.into()).unwrap());
    let mut services = Services {
        loader: BasicLoadBackend::new(
            assets.clone(),
            synthetic.clone(),
            LoadCounters::new(0, [0; 48]),
        ),
    };
    let mut runner = shin_tasks::block_on(Gerokasu2Runner::load(
        assets,
        synthetic,
        Arc::new(AudioManager::new_for_tests()),
        0,
        42,
        InitialMessage {
            style: 0,
            asynchronous: false,
            face: -1,
        },
        Default::default(),
    ))
    .unwrap();
    assert_eq!(
        runner.pump(false, 10, 10000, &mut services).unwrap(),
        RunnerStatus::Exited
    );
    assert_eq!(runner.completed_commands(), 2);
    assert_eq!(services.loader.loaded_notifications().len(), 1);
    assert!(
        services
            .loader
            .layer(
                plane,
                runner.layers().target(LayerId::new(3)).unwrap().layerbank
            )
            .is_some()
    );
    println!(
        "GEROKASU2_LOAD_GPU retained_tile_PIC_UTF8_BUP_reuse_keep_reset_error_explicit_retry_transition_gate_runner_real_publication=verified frames=1 channels=921600 audio=Loopback Audio"
    );
}

mod audio_scene;
mod card_select;
mod controls;
mod event_variables;
mod gallery_scene;
mod layer_edits;
mod movie;
mod root_scene;
mod save_scene;
mod scene;
mod transitions;
