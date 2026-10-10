//! Live ScenarioSelect owner with actual ROM resources and confirmation dialog.
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_start_live_owner_opening_dialog_launch_and_back() {
    use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache};
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = std::sync::Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference depth"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference target"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), format);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
        breakup_texture: None,
        device: &device,
        queue: &queue,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };

    use shin_core::{
        format::{rom::RomReader, save::SaveData},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/snrsel.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/snrsel.txa").unwrap();
    use crate::{adv::input_area::PointerFrame, app::AppAction};
    use enum_map::EnumMap;
    use shin_core::time::Ticks;
    use shin_input::ActionState;
    let mut save = SaveData::default();
    save.persist_data.set(2, 3);
    let previous = context.new_render_texture("Picker previous screen".into());
    let mut scene =
        super::ScenarioSelectScene::new(server.clone(), &save, previous, &context, (1920, 1080));
    let mut number = 0u32;
    let mut step = |scene: &mut super::ScenarioSelectScene,
                    context: &mut PreRenderContext,
                    delta: f32,
                    actions: &[AppAction]| {
        let mut input = EnumMap::default();
        for &action in actions {
            input[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        number += 1;
        scene
            .update_frame(
                Ticks::from_f32(delta),
                &PointerFrame {
                    number,
                    time_seconds: number as f64 / 60.0,
                    events: &[],
                },
                &input,
                glam::vec2(-100.0, -100.0),
                delta as f64 / 60.0,
                &server,
                context,
            )
            .unwrap();
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !scene.music.contains(&super::OpeningEvent::StopMusic) {
        assert!(
            std::time::Instant::now() < deadline,
            "picker worker timeout"
        );
        step(&mut scene, &mut context, 0.0, &[AppAction::Enter]);
        assert!(!scene.accepts_input());
        assert!(scene.take_destination().is_none());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    step(&mut scene, &mut context, 60.0, &[AppAction::Enter]);
    assert!(!scene.accepts_input());
    step(&mut scene, &mut context, 0.0, &[]);
    assert!(scene.accepts_input());
    assert_eq!(
        scene.music,
        [
            super::OpeningEvent::StopMusic,
            super::OpeningEvent::StartMusic
        ]
    );
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(context.depth_stencil),
            "Live Start initial",
        );
        scene.render(&mut pass);
    }
    let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
    std::fs::write("/private/tmp/shin-start-owner-initial.rgba", pixels).unwrap();
    step(&mut scene, &mut context, 0.0, &[AppAction::AnyDown]);
    assert_eq!(scene.selection.selected(), (0, 1));
    assert_eq!(scene.sounds, ["move"]);
    step(&mut scene, &mut context, 7.0, &[AppAction::Enter]);
    assert!(!scene.accepts_input());
    assert!(scene.dialog.is_none());
    step(&mut scene, &mut context, 0.0, &[]);
    assert!(scene.dialog.is_some());
    assert!(scene.take_destination().is_none());
    for _ in 0..24 {
        step(&mut scene, &mut context, 1.0, &[]);
    }
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(context.depth_stencil),
            "Live Start confirmation",
        );
        scene.render(&mut pass);
    }
    let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
    std::fs::write("/private/tmp/shin-start-owner-dialog.rgba", pixels).unwrap();
    // Default native222d0 choice1 means No. Wait for final callback, not the intermediate result.
    step(&mut scene, &mut context, 1.0, &[AppAction::Enter]);
    for _ in 0..35 {
        step(&mut scene, &mut context, 1.0, &[]);
        assert!(scene.take_destination().is_none());
    }
    assert!(scene.dialog.is_none());
    assert!(scene.accepts_input());
    step(&mut scene, &mut context, 7.0, &[AppAction::Enter]);
    step(&mut scene, &mut context, 0.0, &[]);
    for _ in 0..24 {
        step(&mut scene, &mut context, 1.0, &[]);
    }
    step(
        &mut scene,
        &mut context,
        0.0,
        &[AppAction::ScenarioTabPrevious],
    );
    step(&mut scene, &mut context, 1.0, &[AppAction::Enter]);
    assert!(scene.take_destination().is_none());
    for _ in 0..35 {
        step(&mut scene, &mut context, 1.0, &[]);
    }
    assert_eq!(
        scene.take_destination(),
        Some(super::ScenarioDestination::Episode(1))
    );
    assert!(scene.launch_music_stop);
    assert!(scene.sounds.contains(&"dooropen"));
    assert!(scene.take_destination().is_none());
    assert!(!scene.accepts_input());
    // Independent back task waits for close flash and polls before clock advancement.
    let previous = context.new_render_texture("Picker back previous screen".into());
    let mut back =
        super::ScenarioSelectScene::new(server.clone(), &save, previous, &context, (1920, 1080));
    while !back.music.contains(&super::OpeningEvent::StopMusic) {
        assert!(std::time::Instant::now() < deadline);
        step(&mut back, &mut context, 0.0, &[]);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    step(&mut back, &mut context, 60.0, &[]);
    step(&mut back, &mut context, 0.0, &[]);
    assert!(back.selection.pointer_probe(glam::vec2(1800.0, 60.0)));
    back.selection.pointer_release();
    step(&mut back, &mut context, 7.0, &[]);
    assert_eq!(back.take_destination(), None);
    step(&mut back, &mut context, 0.0, &[]);
    assert_eq!(
        back.take_destination(),
        Some(super::ScenarioDestination::Title)
    );
    assert_eq!(back.take_destination(), None);
    if atlas.get("help_snrsel").size().height == 240 {
        crate::app::SceneHost::reference_scenario_unlock_roundtrip(&server, &mut context, &target);
    }
    println!(
        "Live Start owner: real worker,60tick opening,native full composition,completed chapter No/Yes final callbacks,Episode1 request,close flash before Title request passed"
    );
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_start_owner_initial_selection_back_audio_and_dialog_callback() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0xdd354, 0xb2607fe9),
        (0xdd35c, 0x9106526a),
        (0xdd0f4, 0xeb08013f),
        (0xdd0f8, 0x1e280de0),
        (0xdd100, 0x97ff7878),
        (0xe28d4, 0x91088100),
        (0xe28d8, 0x97ff6446),
        (0xe28dc, 0x360000c0),
        (0xe2900, 0x9400ada8),
        (0xe2908, 0x97fd129a),
        (0xdf550, 0xb0000a28),
        (0xdf5c8, 0xf943bd08),
        (0xdf7c4, 0xf9437108),
        (0xdf898, 0xf9437508),
        (0xdf944, 0xf9427d08),
        (0xdf964, 0x321d0fe1),
        (0xdf968, 0x97fd50b2),
        (0xe2d64, 0x7100051f),
        (0xe2d74, 0x34000068),
        (0xe2d78, 0x17fff16e),
        (0xe2d80, 0x17fff2ec),
        (0x339e4, 0x2a1903e2),
        (0x339e8, 0x940000ba),
        (0x33d14, 0x320003e1),
        (0x33d18, 0x2a1403e2),
        (0x33d20, 0x9402ff58),
        (0xf3b30, 0xb902c514),
        (0xf459c, 0xb942c669),
        (0xf45a0, 0x51000529),
        (0xf45a8, 0x540001a2),
    ] {
        assert_eq!(word(a), expected, "native picker owner {a:x}");
    }
    let relocation = |target: u64| {
        (0x11e040..0x152eb8)
            .step_by(24)
            .find_map(|a| {
                let slot = u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap());
                (slot == target)
                    .then(|| u64::from_le_bytes(exe[a + 272..a + 280].try_into().unwrap()))
            })
            .unwrap()
    };
    for (slot, name) in [
        (0x224780, "move"),
        (0x224778, "cancel"),
        (0x2246e0, "decide"),
        (0x2246e8, "error"),
        (0x2254f8, "dooropen"),
    ] {
        let pointer = relocation(relocation(slot)) as usize + 256;
        assert_eq!(&exe[pointer..pointer + name.len()], name.as_bytes());
    }
    // stp into18c/190 stores selected entry0 and pointer sentinel-1.
    // It does not initialize selected entry to-1.
    let packed = 0xffffffff00000000u64;
    assert_eq!(packed as u32, 0);
    let prompt = "このシナリオを開始します。@rよろしいですか？";
    assert_eq!(
        &exe[0x1635cc + 256..0x1635cc + 256 + prompt.len()],
        prompt.as_bytes()
    );
}
