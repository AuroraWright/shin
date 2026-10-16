use super::*;
use crate::{
    app::gerokasu2::{
        load_menu::LoadMenuHost,
        tests::{assets, bytes, msg, num},
    },
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, LayeredAssetIo},
    },
};
use enum_map::EnumMap;
use glam::{Mat4, Vec2};
use shin_core::format::save::gerokasu2::{GameData, Progress, SaveDate};
use shin_render::{
    ColorBlendType, RenderRequestBuilder,
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    quad_vertices::QuadVertices,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};
use std::time::{Duration, Instant};

struct Frames {
    resize: SurfaceResizeSource,
    depth: wgpu::TextureView,
    color: RenderTexture,
    samplers: TextureSamplerStore,
    pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    rendered: usize,
}
impl Frames {
    fn new(gpu: &Offscreen) -> Self {
        let resize = SurfaceResizeSource::new(ViewportParams::both((320, 180).into()));
        Self {
            depth: capture::depth(&gpu.device, [320, 180]),
            color: RenderTexture::new(
                gpu.device.clone(),
                resize.handle(),
                "Preview menu test".into(),
            ),
            resize,
            samplers: TextureSamplerStore::new(&gpu.device),
            pipelines: PipelineStorage::new(gpu.device.clone(), shin_render::TEXTURE_FORMAT),
            dynamic: DynamicBuffer::new(gpu.device.clone(), BytesAddress::new(2 * 1024 * 1024)),
            rendered: 0,
        }
    }
    fn step(&mut self, gpu: &Offscreen, menu: &mut LoadMenuHost) {
        assert!(
            menu.update(
                1.,
                &EnumMap::default(),
                Vec2::ZERO,
                crate::app::gerokasu2::Viewport {
                    offset: Vec2::ZERO,
                    size: Vec2::new(1920., 1080.)
                },
                self.rendered as f64 / 60.
            )
            .is_none()
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut ctx = PreRenderContext {
            device: &gpu.device,
            queue: &gpu.queue,
            breakup_texture: None,
            resize_source: &self.resize,
            sampler_store: &self.samplers,
            depth_stencil: DepthStencilTarget { view: &self.depth },
            pipeline_storage: &mut self.pipelines,
            dynamic_buffer: &mut self.dynamic,
            encoder: &mut encoder,
        };
        menu.prepare(&mut ctx).unwrap();
        {
            let mut pass = ctx.begin_pass(
                self.color.as_texture_target(),
                Some(ctx.depth_stencil),
                "Actual SaveMenu thumbnails",
            );
            menu.render(&mut pass).unwrap();
        }
        let mut uploads = gpu.device.create_command_encoder(&Default::default());
        self.dynamic.finish(&mut uploads);
        gpu.queue.submit([uploads.finish(), encoder.finish()]);
        self.dynamic.recall();
        gpu.device.poll(wgpu::PollType::Poll).unwrap();
        self.rendered += 1;
        std::thread::sleep(Duration::from_millis(1));
    }
    fn ready(&mut self, gpu: &Offscreen, menu: &mut LoadMenuHost, slots: &[usize]) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while slots.iter().any(|&slot| {
            menu.previews.textures[slot].is_none() || menu.previews.labels[slot].is_none()
        }) {
            self.step(gpu, menu);
            assert!(Instant::now() < deadline, "preview jobs did not finish");
        }
    }
}

fn pixels(gpu: &mut Offscreen, texture: &RenderTexture) -> Vec<[u8; 4]> {
    gpu.draw(|pass| {
        QuadVertices::new()
            .with_box(-1., 1., 1., -1.)
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                texture.as_texture_source(),
                Mat4::IDENTITY,
            )
    })
}
fn text(code: &mut Vec<u8>, level: i32, value: &str) {
    code.push(0xa0);
    num(code, level);
    code.extend(((value.len() + 1) as u16).to_le_bytes());
    code.extend(value.as_bytes());
    code.push(0);
}
fn layer(code: &mut Vec<u8>, kind: i32, parameters: [i32; 8]) {
    code.push(0xc1);
    for value in [0, kind, 0] {
        num(code, value)
    }
    code.push(0xff);
    for value in parameters {
        num(code, value)
    }
}
fn data() -> SaveData {
    let mut data = SaveData::default();
    for (slot, target) in [(0, 2), (1, 4), (2, 6)] {
        let mut progress = Progress::fresh(0, 42);
        progress.active_entry_mut().save_position = target;
        progress.active_entry_mut().replay_target = target;
        data.manual_save_slots[slot] = Some(GameData {
            date: SaveDate {
                year: 2026,
                month: 10,
                day: 15,
                ..Default::default()
            },
            progress,
        });
    }
    data
}

#[test]
#[ignore = "actual SaveMenu saved-script replay/private resource scene/thumbnails/scroll invalidation and worker failure on Metal/exact Loopback Audio; temporary missing-assets directory only"]
fn ger_save_previews_render_private_scenes_retain_snapshots_and_retire_textures() {
    let mut gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    // The supplied movies have no usable linked picture. Extend only this
    // synthetic in-memory scenario to exercise native kind6 -> kind2 with a
    // real ROM picture; supplied scenario bytes/ROM remain unchanged.
    assert!(!original.movie_info.is_empty());
    assert!(!original.picture_info.is_empty());
    let movie = original.movie_info.len();
    let picture = 0i16;
    let mut code = bytes();
    code.truncate(original.header.code_offset as usize);
    text(&mut code, 0, "第一章");
    text(&mut code, 1, "白い場面");
    layer(&mut code, 1, [0xffff, -960, -540, 1920, 1080, 0, 0, 0]);
    code.push(0xa1);
    msg(&mut code, 1, "@r保存した白い場面。@k");
    text(&mut code, 1, "赤い場面");
    layer(&mut code, 1, [0xff00, -960, -540, 1920, 1080, 0, 0, 0]);
    code.push(0xa1);
    msg(&mut code, 2, "@r保存した赤い場面。@k");
    text(&mut code, 0, "");
    text(&mut code, 1, "静止画");
    layer(&mut code, 6, [movie as i32, 1000, 1, 0, 0, 0, 0, 0]);
    code.push(0xa1);
    msg(&mut code, 3, "@r動画はサムネイルでは静止画。@k");
    code.extend([0, 0, 0]);
    let size = code.len() as u32;
    code[4..8].copy_from_slice(&size.to_le_bytes());
    let mut scenario = Gerokasu2Scenario::new(code.into()).unwrap();
    scenario
        .movie_info
        .push(shin_core::format::scenario::gerokasu2::MovieInfoItem {
            name: shin_core::format::text::utf8::U16Utf8String::new("preview-fixture"),
            linked_picture_id: picture,
            volume_source: 0,
            transparency: 0,
            linked_bgm_id: -1,
        });
    let mut scenario = Arc::new(scenario);
    let assets = assets(&gpu);
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut data = data();
    let mut menu = shin_tasks::block_on(LoadMenuHost::load(
        assets.clone(),
        scenario.clone(),
        audio.clone(),
        &data,
    ))
    .unwrap();
    let mut frames = Frames::new(&gpu);
    frames.ready(&gpu, &mut menu, &[0, 1, 2]);
    assert_eq!(menu.previews.replay_starts[..3], [1, 1, 1]);
    assert_eq!(menu.previews.texture_starts[..3], [1, 1, 1]);
    for slot in 0..3 {
        assert_eq!(
            menu.previews.textures[slot].as_ref().unwrap().dimensions(),
            (336, 189)
        );
    }
    assert!(
        pixels(&mut gpu, menu.previews.textures[0].as_ref().unwrap())
            .iter()
            .all(|p| *p == [255, 255, 255, 255])
    );
    let red_pixels = pixels(&mut gpu, menu.previews.textures[1].as_ref().unwrap());
    assert!(
        red_pixels.iter().all(|p| *p == [255, 0, 0, 255]),
        "second scene pixels: {red_pixels:?}"
    );
    let linked = pixels(&mut gpu, menu.previews.textures[2].as_ref().unwrap());
    assert!(linked.iter().any(|p| p[..3] != [0, 0, 0]));
    assert_eq!(
        menu.previews.snapshots[0].as_ref().unwrap().labels[0].as_ref(),
        "第一章"
    );
    assert_eq!(
        menu.previews.snapshots[0].as_ref().unwrap().labels[1].as_ref(),
        "白い場面"
    );
    assert!(menu.previews.labels[0].as_ref().unwrap().subtitle.is_some());
    assert!(menu.previews.labels[2].as_ref().unwrap().subtitle.is_none());
    let bank = shin_core::vm::command::types::LayerbankId::new(0);
    assert_eq!(
        menu.previews.snapshots[2]
            .as_ref()
            .unwrap()
            .layers
            .record(bank)
            .kind(),
        6
    );
    // Replaying this scene retains movie kind6 in the snapshot; only the
    // private resource worker converts it to the linked picture.
    let current = menu.previews.textures[2].as_ref().unwrap();
    let last = Arc::downgrade(current);
    let first = Arc::downgrade(menu.previews.textures[0].as_ref().unwrap());
    menu.model.drag_scroll(2000.);
    frames.step(&gpu, &mut menu);
    assert!(menu.previews.textures[..3].iter().all(Option::is_none));
    assert!(menu.previews.labels[..3].iter().all(Option::is_none));
    assert!(menu.previews.snapshots[..3].iter().all(Option::is_some));
    assert_eq!(first.strong_count(), 0);
    assert_eq!(
        last.strong_count(),
        1,
        "last texture service is retained while abandoned"
    );
    menu.model.drag_scroll(0.);
    frames.ready(&gpu, &mut menu, &[0, 1, 2]);
    assert_eq!(menu.previews.replay_starts[..3], [1, 1, 1]);
    assert_eq!(menu.previews.texture_starts[..3], [2, 2, 2]);
    assert_eq!(last.strong_count(), 0);
    assert_eq!(
        pixels(&mut gpu, menu.previews.textures[2].as_ref().unwrap()),
        linked
    );
    // Overwriting one slot invalidates only its caches and leaves another
    // visible row's existing resource owner untouched.
    let retained = Arc::downgrade(menu.previews.textures[1].as_ref().unwrap());
    data.manual_save_slots[0].as_mut().unwrap().progress =
        data.manual_save_slots[1].as_ref().unwrap().progress.clone();
    menu.refresh_slots(&data);
    frames.ready(&gpu, &mut menu, &[0, 1, 2]);
    assert_eq!(menu.previews.replay_starts[..3], [2, 1, 1]);
    assert_eq!(menu.previews.texture_starts[..3], [3, 2, 2]);
    assert!(std::ptr::eq(
        retained.as_ptr(),
        Arc::as_ptr(menu.previews.textures[1].as_ref().unwrap())
    ));
    assert!(
        pixels(&mut gpu, menu.previews.textures[0].as_ref().unwrap())
            .iter()
            .all(|p| *p == [255, 0, 0, 255])
    );
    // Invalidate an active replay, without cancellation. Its old payload must
    // be discarded before the replacement record can publish.
    menu.previews.invalidate(0);
    menu.previews.update(0., &data);
    assert_eq!(menu.previews.replay_starts[0], 3);
    data.manual_save_slots[0]
        .as_mut()
        .unwrap()
        .progress
        .active_entry_mut()
        .save_position = 2;
    data.manual_save_slots[0]
        .as_mut()
        .unwrap()
        .progress
        .active_entry_mut()
        .replay_target = 2;
    menu.refresh_slots(&data);
    frames.ready(&gpu, &mut menu, &[0, 1, 2]);
    assert_eq!(menu.previews.replay_starts[..3], [4, 1, 1]);
    assert!(
        pixels(&mut gpu, menu.previews.textures[0].as_ref().unwrap())
            .iter()
            .all(|p| *p == [255, 255, 255, 255])
    );
    let final_owner = Arc::downgrade(menu.previews.textures[0].as_ref().unwrap());
    drop(menu);
    assert_eq!(final_owner.strong_count(), 0);

    // Valid movies with a negative linked-picture ID become a Null drawable.
    // They must not retain pixels from a prior scene's reused intermediates.
    data.manual_save_slots[0] = None;
    data.manual_save_slots[1] = None;
    Arc::get_mut(&mut scenario)
        .expect("menu jobs must retire scenario ownership")
        .movie_info[movie]
        .linked_picture_id = -1;
    let mut no_still = shin_tasks::block_on(LoadMenuHost::load(
        assets.clone(),
        scenario.clone(),
        audio.clone(),
        &data,
    ))
    .unwrap();
    frames.ready(&gpu, &mut no_still, &[2]);
    assert!(
        pixels(&mut gpu, no_still.previews.textures[2].as_ref().unwrap())
            .iter()
            .all(|p| *p == [0, 0, 0, 255])
    );
    drop(no_still);
    Arc::get_mut(&mut scenario).unwrap().movie_info[movie].linked_picture_id = picture;

    // Real asset lookup failure completes the texture service with its native
    // opaque-black fallback; it does not replace the menu or saved snapshot.
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let folder =
        Temporary(std::env::temp_dir().join(format!("shin-ger-preview-{}", std::process::id())));
    std::fs::create_dir(&folder.0).unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&folder.0).unwrap();
    let missing = Arc::new(AssetServer::new(
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
    data.manual_save_slots[0] = None;
    data.manual_save_slots[1] = None;
    let mut failed =
        shin_tasks::block_on(LoadMenuHost::load(assets, scenario, audio, &data)).unwrap();
    failed.previews.assets = missing;
    frames.ready(&gpu, &mut failed, &[2]);
    assert!(
        pixels(&mut gpu, failed.previews.textures[2].as_ref().unwrap())
            .iter()
            .all(|p| *p == [0, 0, 0, 255])
    );
    assert!(failed.previews.snapshots[2].is_some());
    drop(failed);
    println!(
        "GER_SAVE_PREVIEW_HOST rows=3 movie={movie} linked_picture={picture} captures=10 menu_frames={} private_replay_two_workers_real_assets_three_stage_capture_hidden_cache_slot_invalidation_discard_black_fallback_retirement=verified audio=Loopback Audio",
        frames.rendered
    );
}
