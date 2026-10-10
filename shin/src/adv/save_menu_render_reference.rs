//! Supplied atlas GPU verification of native SaveMenu window slices and live artwork.
use super::{
    character_view::CharacterTextures,
    save_menu::{SaveMenuInput, SaveMenuModel},
    save_menu_text::{SaveSlotLabels, SaveSlotText},
    save_menu_view,
};
use crate::render::PreRenderContext;
use shin_core::{format::save::SaveData, primitives::color::UnormColor, time::Ticks};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_save_menu_gpu_window_and_artwork() {
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
        label: Some("Save menu reference depth"),
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
        label: Some("Save menu reference target"),
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

    if std::env::var_os("SHIN_REFERENCE_MURDERER_CAPTURE_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_capture_lifecycle(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_ACCUSATION_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_accusation_draw(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_PURPLE_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_purple_draw(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_HINTS_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_hints_draw(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_REPLAY_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_replay_draw(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_TOP_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_top_draw(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_RULE_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_rule_draw(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_FAST_FORWARD_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::murderer_accusation::reference_fast_forward(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MURDERER_PARENT_ONLY").is_some() {
        let _audio = shin_audio::AudioManager::new_for_tests();
        crate::layer::user::reference_parent_properties_and_capture(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_BGM_TITLE_ONLY").is_some() {
        let scenario = shin_tasks::block_on(
            server.load::<shin_core::format::scenario::Scenario, _>("/main.snr"),
        )
        .unwrap();
        super::bgm_title::BgmTitle::assert_reference_render(&mut context, &server, &scenario);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_ENDING_ONLY").is_some() {
        super::ending_reference::assert_ending_returns_to_title(&mut context, &server, &target);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MSGSET_ONLY").is_some() {
        crate::layer::message_layer::reference_live_msgset(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_EVENT_SKIP_ONLY").is_some() {
        super::event_skip::assert_reference_render_and_live(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_SKIP_INDICATOR_ONLY").is_some() {
        super::skip_indicator::assert_reference_render(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_TEXT_COLORS_ONLY").is_some() {
        crate::layer::message_layer::reference_authored_truth_color_render(&mut context, &server);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_PURPLE_COLORS_ONLY").is_some() {
        crate::layer::user::reference_transcript_colors_render(&mut context, &server);
        return;
    }
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/saveload.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/saveload.txa").unwrap();
    let system: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();
    let sample = |name: &str, u: f32, v: f32| -> [f32; 4] {
        let im = &original.textures[original.name_to_index[name]];
        let x = u * im.width() as f32 - 0.5;
        let y = v * im.height() as f32 - 0.5;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        std::array::from_fn(|channel| {
            let p = |dx: f32, dy: f32| {
                im.get_pixel(
                    (ix + dx).clamp(0.0, im.width() as f32 - 1.0) as u32,
                    (iy + dy).clamp(0.0, im.height() as f32 - 1.0) as u32,
                )[channel] as f32
                    / 255.0
            };
            (p(0.0, 0.0) * (1.0 - fx) + p(1.0, 0.0) * fx) * (1.0 - fy)
                + (p(0.0, 1.0) * (1.0 - fx) + p(1.0, 1.0) * fx) * fy
        })
    };

    {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(context.depth_stencil),
            "SaveMenu native window oracle",
        );
        pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
        for draw in save_menu_view::window_slices() {
            draw.render(
                &atlas,
                &mut pass,
                crate::render::top_left_projection_matrix(),
                shin_render::RenderRequestBuilder::new(),
            );
        }
    }
    let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
    let mut checked = 0;
    let mut max_error = 0.0f32;
    // Independent transcription of d7de0 and16ddc0. Center is opaque.
    for y in (110..1013).step_by(3) {
        for x in (45..1875).step_by(3) {
            let px = x as f32 + 0.5 - 48.0;
            let py = y as f32 + 0.5 - 112.0;
            let mut expected = [0.0f32; 3];
            if (0.0..1824.0).contains(&px) && (0.0..898.0).contains(&py) {
                let source = sample("window", px / 1824.0, py / 898.0);
                let center = (80.0..1744.0).contains(&px) && (80.0..810.0).contains(&py);
                for c in 0..3 {
                    expected[c] = source[c] * if center { 1.0 } else { source[3] };
                }
            }
            let offset = (y * 1920 + x) * 4;
            for c in 0..3 {
                let error = (pixels[offset + c] as f32 - expected[c] * 255.0).abs();
                max_error = max_error.max(error);
                assert!(
                    error <= 2.0,
                    "SaveMenu window{x},{y} channel{c}: GPU{} scalar{}",
                    pixels[offset + c],
                    expected[c] * 255.0
                );
            }
            checked += 1;
        }
    }
    let labels = [
        SaveSlotLabels::Empty,
        SaveSlotLabels::Loading,
        SaveSlotLabels::Ready {
            title: "Episode1 Legend of the golden witch".into(),
            subtitle: "10/4（土）8:00 新島空港到着".into(),
        },
    ];
    let texts: Vec<_> = labels
        .into_iter()
        .map(|labels| SaveSlotText::load(&server, labels).unwrap())
        .collect();
    let mut changed_total = 0;
    for save in [true, false] {
        for case in 0..3 {
            let mut data = SaveData::default();
            data.save_menu_position = if case == 2 { 99 } else { 0 };
            let entry = crate::adv::VmState::for_scenario(7, 42).snapshot_game_data();
            let mut model = SaveMenuModel::new(&data, save.then_some(entry));
            model.assets_ready();
            model.update(if case == 0 { 5.0 } else { 10.0 });
            if case == 2 {
                model.input(SaveMenuInput {
                    confirm: true,
                    ..Default::default()
                });
                model.update(2.0);
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "SaveMenu artwork composition",
                );
                pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
                save_menu_view::render(
                    &model,
                    &atlas,
                    &system,
                    &mut pass,
                    crate::render::top_left_projection_matrix(),
                    |_, _, _, _| false,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            assert!(
                pixels
                    .chunks_exact(4)
                    .filter(|p| p[0] > 32 || p[1] > 32 || p[2] > 32)
                    .count()
                    > 100000
            );
            if save && case == 1 {
                std::fs::write("/private/tmp/shin-save-menu-artwork.rgba", &pixels).unwrap();
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "SaveMenu supplied font and stencil",
                );
                pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
                save_menu_view::render(
                    &model,
                    &atlas,
                    &system,
                    &mut pass,
                    crate::render::top_left_projection_matrix(),
                    |pass, slot, transform, builder| {
                        texts[slot % texts.len()].render(pass, transform, builder);
                        false
                    },
                );
            }
            let text_pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let mut changed = 0;
            for (i, (before, after)) in pixels
                .chunks_exact(4)
                .zip(text_pixels.chunks_exact(4))
                .enumerate()
            {
                if before != after {
                    changed += 1;
                    let x = (i % 1920) as f32 + 0.5;
                    let y = (i / 1920) as f32 + 0.5;
                    let left = 136.0 + 1920.0 * (1.0 - model.progress());
                    assert!(
                        x >= left && x < left + 1648.0 && (183.0..923.0).contains(&y),
                        "slot glyph escaped content stencil at {x},{y}"
                    );
                }
            }
            assert!(changed > 1000, "slot labels did not draw: {changed} pixels");
            changed_total += changed;
            if save && case == 1 {
                std::fs::write("/private/tmp/shin-save-menu-text.rgba", text_pixels).unwrap();
            }
        }
    }
    super::save_menu_screen::assert_reference_screen(&mut context, &target, &server);
    super::save_menu_preview::assert_reference_preview(&mut context, &target, &server);
    eprintln!("SaveMenu slot labels: {changed_total} changed pixels within native content stencil");
    eprintln!(
        "SaveMenu window: {checked} independent samples, max RGB error{max_error}; six actual GPU artwork and supplied-font compositions (scene previews supplied by future screen)"
    );
}

#[test]
#[ignore = "requires supplied ROM, Metal GPU and SHIN_REFERENCE_BGM_TITLE_ONLY"]
fn reference_authored_bgm_title_unicode_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_BGM_TITLE_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM and Metal GPU; set SHIN_REFERENCE_SKIP_INDICATOR_ONLY=1"]
fn reference_skip_indicator_actual_adv_root_and_native_artwork_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_SKIP_INDICATOR_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_EVENT_SKIP_ONLY"]
fn reference_event_skip_artwork_and_live_movie() {
    assert!(std::env::var_os("SHIN_REFERENCE_EVENT_SKIP_ONLY").is_some());
    std::thread::Builder::new()
        .name("event-skip-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROM, copied Episode8 progress, Metal and Loopback Audio"]
fn reference_episode8_trick_ending_returns_to_title() {
    assert!(std::env::var_os("SHIN_REFERENCE_ENDING_ONLY").is_some());
    std::thread::Builder::new()
        .name("ending-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_MSGSET_ONLY"]
fn reference_msgset_install_skip_read_transitions_and_waits() {
    assert!(std::env::var_os("SHIN_REFERENCE_MSGSET_ONLY").is_some());
    std::thread::Builder::new()
        .name("msgset-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_TEXT_COLORS_ONLY"]
fn reference_authored_red_truth_and_blue_text_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_TEXT_COLORS_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM, Metal GPU and SHIN_REFERENCE_PURPLE_COLORS_ONLY"]
fn reference_murderer_red_and_purple_transcript_colors_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_PURPLE_COLORS_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_MURDERER_PARENT_ONLY"]
fn reference_murderer_parent_properties_capture_and_retained_wipes_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_PARENT_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_MURDERER_FAST_FORWARD_ONLY"]
fn reference_murderer_fast_forward_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_FAST_FORWARD_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal, Loopback Audio and SHIN_REFERENCE_MURDERER_RULE_ONLY"]
fn reference_murderer_native_rule_child_draw_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_RULE_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal, Loopback Audio and SHIN_REFERENCE_MURDERER_TOP_ONLY"]
fn reference_murderer_native_top_child_draw_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_TOP_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_MURDERER_REPLAY_ONLY"]
fn reference_murderer_native_replay_child_draw_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_REPLAY_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM, Metal, Loopback Audio and SHIN_REFERENCE_MURDERER_HINTS_ONLY"]
fn reference_murderer_native_hints_child_draw_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_HINTS_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM/Metal/Loopback and SHIN_REFERENCE_MURDERER_CAPTURE_ONLY"]
fn reference_murderer_native_capture_lifecycle_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_CAPTURE_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM/Metal/Loopback and SHIN_REFERENCE_MURDERER_PURPLE_ONLY"]
fn reference_murderer_native_purple_child_draw_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_PURPLE_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}

#[test]
#[ignore = "requires supplied ROM/Metal/Loopback and SHIN_REFERENCE_MURDERER_ACCUSATION_ONLY"]
fn reference_murderer_native_accusation_child_draw_gpu() {
    assert!(std::env::var_os("SHIN_REFERENCE_MURDERER_ACCUSATION_ONLY").is_some());
    reference_save_menu_gpu_window_and_artwork();
}
