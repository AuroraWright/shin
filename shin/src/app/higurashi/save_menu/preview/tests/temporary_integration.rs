use super::super::*;
use super::{data, scenario};
use crate::adv::higurashi::{
    browser_controls::Scrollbar, reference_gpu_assets, reference_render_to_texture,
    save_menu::model::Mode,
};
use binrw::BinWrite;
use shin_core::{
    format::{save::higurashi::SaveData, scenario::instruction_elements::UntypedNumberSpec},
    primitives::color::UnormColor,
};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    cell::RefCell,
    io::{Cursor, Write as _},
    process::{Command, Stdio},
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use temporary::tests::Reference;

fn number(code: &mut Vec<u8>, value: i32) {
    let mut bytes = Cursor::new(Vec::new());
    UntypedNumberSpec::Constant(value)
        .write_le(&mut bytes)
        .unwrap();
    code.extend(bytes.into_inner());
}
fn program(kind: i32, ids: &[usize], prefix_null: bool) -> Vec<u8> {
    let mut code = Vec::new();
    if prefix_null {
        // A retained basic Null occupies the first physical slot. Its clone
        // provides a deterministic barrier before the following TIPS clone.
        code.extend([0xc1, 7, 0, 0, 0]);
    }
    code.extend([0xa0, 0, 2, b'A', 0, 0xa0, 1, 2, b'B', 0]);
    if kind == 11 {
        code.extend([0xd1, 18, 0, 31, 0, 7]);
    } else {
        code.push(0xd0);
        code.push(ids.len() as u8);
        for &id in ids {
            number(&mut code, id as i32);
        }
    }
    code.extend([0, 0, 0]);
    code
}
fn state() -> ViewState {
    ViewState {
        mode: Mode::Load,
        selected: 0,
        progress: 1.,
        scroll: 0.,
        close_flash: 0.,
        row_flash: [0.; 100],
        scrollbar: Scrollbar {
            position: 0,
            visible: 818,
            first: 0,
            end: 20618,
        },
    }
}
fn acquired(words: &[u32], id: usize) -> bool {
    let bit = 2 * id;
    words
        .get(bit / 32)
        .is_some_and(|v| v & (1 << (bit % 32)) != 0)
}
fn set(words: &mut [u32], id: usize) {
    let bit = 2 * id;
    words[bit / 32] |= 1 << (bit % 32);
}
fn settings() -> SaveData {
    let mut settings = SaveData::default();
    settings.settings.common.v14_showroutenavi = false;
    settings.save_vectors.extra_words_0 = vec![0; 20];
    settings
}
fn until(mut ready: impl FnMut() -> bool, label: &str) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < Duration::from_secs(30), "{label} timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn wait_ready(service: &mut PreviewService, slots: &Slots, state: &ViewState) {
    until(
        || {
            service.poll_rows(slots, state).unwrap();
            service.capture.as_ref().is_some_and(|c| c.ready.is_some())
        },
        "Retained temporary capture",
    );
}
fn actual_thumbnail(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    scene: &Scene,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |ctx, view| {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view,
            },
            Some(ctx.depth_stencil),
            "Retained temporary Scene group",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        scene.render(&mut pass).unwrap();
    })
}
fn original_thumbnail(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    native: &Reference,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |ctx, view| {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view,
            },
            Some(ctx.depth_stencil),
            "Original temporary packets at single-child group stencil",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        native.render(&mut pass, 1, None);
    })
}
fn native_capture(
    ctx: &mut PreRenderContext,
    canvas: (u32, u32),
    native: &Reference,
) -> RenderTexture {
    let (width, height) = resources::thumbnail_size(canvas);
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        width, height,
    )));
    let depth = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Original temporary thumbnail depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth = depth.create_view(&Default::default());
    let mut capture = PreRenderContext {
        device: ctx.device,
        queue: ctx.queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: ctx.sampler_store,
        depth_stencil: DepthStencilTarget { view: &depth },
        pipeline_storage: ctx.pipeline_storage,
        dynamic_buffer: ctx.dynamic_buffer,
        encoder: ctx.encoder,
    };
    let mut texture = capture.new_render_texture("Original refreshed temporary thumbnail".into());
    {
        let mut pass = capture.begin_pass(
            texture.as_texture_target(),
            Some(capture.depth_stencil),
            "Original temporary capture",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        native.render(&mut pass, 1, None);
    }
    texture
}
fn present(ctx: &mut PreRenderContext, view: &wgpu::TextureView, texture: &RenderTexture) {
    let mut pass = ctx.begin_pass(
        TextureTarget {
            kind: TextureTargetKind::Screen,
            view,
        },
        Some(ctx.depth_stencil),
        "Retained temporary thumbnail publication",
    );
    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
    QuadVertices::new()
        .with_box(4., 162., 346., 354.)
        .with_tex_box(0., 0., 1., 1.)
        .render_sprite(
            &mut pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
            texture.as_texture_source(),
            crate::render::top_left_projection_matrix(),
        );
}

fn native_group(scene: &Scene) -> Vec<(u8, u8, PassKind)> {
    use group::Drawable as _;
    let mut input = format!("G {}", scene.layers.len());
    for (_, id, layer) in &scene.layers {
        input.push_str(&format!(
            " {id} {:08x}",
            layer.properties().values()[5].to_bits()
        ));
    }
    input.push('\n');
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_GROUP_ORDER").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    let mut w = output.split_whitespace();
    assert_eq!(w.next(), Some("G"));
    assert_eq!(
        w.next().unwrap().parse::<usize>().unwrap(),
        scene.layers.len()
    );
    for _ in &scene.layers {
        w.next().unwrap();
    }
    let plan = (0..scene.layers.len() * 2)
        .map(|_| {
            let slot = w.next().unwrap().parse::<usize>().unwrap();
            let stencil = w.next().unwrap().parse().unwrap();
            let kind = match w.next().unwrap() {
                "1" => PassKind::Opaque,
                "0" => PassKind::Transparent,
                p => panic!("Original group pass{p}"),
            };
            (scene.layers[slot].0, stencil, kind)
        })
        .collect();
    assert!(w.next().is_none());
    plan
}
struct Spy<'a> {
    slot: u8,
    layer: &'a Layer,
    visits: &'a RefCell<Vec<(u8, u8, PassKind)>>,
}
impl group::Drawable for Spy<'_> {
    fn properties(&self) -> &HigurashiProperties {
        group::Drawable::properties(self.layer)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> anyhow::Result<()> {
        self.visits.borrow_mut().push((self.slot, stencil, kind));
        group::Drawable::render(self.layer, pass, parent, stencil, kind)
    }
}
fn verify_all_ready_order_and_cancellation(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    server: &AssetServer,
) {
    let mut authored = Arc::try_unwrap(scenario(&program(12, &[0], false))).unwrap();
    // Metadata exists, so actual resource failure cannot become a catalog-miss
    // Null fallback. This remains a real preparation error as factories grow.
    authored.picture_info[0].name.0 = "__missing_saved_preview_fixture__".into();
    assert!(!server.contains("/picture/__missing_saved_preview_fixture__.pic"));
    let scenario = Arc::new(authored);
    let mut saved = Hydrated::replay(&scenario, &data(0, 1, 999, 42)).unwrap();
    let template = saved.records[0];
    for (slot, record) in saved.records.iter_mut().enumerate() {
        *record = template;
        record[1] = (31 - slot) as i32;
        record[14 + 5] = (slot % 3) as i32;
        record[7] = 1;
        record[8] = [0, 17, 244, 314][slot % 4];
    }
    saved.records[0][0] = 0;
    saved.records[31][0] = 2;
    saved.records[31][6] = 0;
    let globals = globals::State::shared(globals::Snapshot::new(&settings()));
    let calls = Arc::new(AtomicU32::new(100));
    let samples = Arc::new(Mutex::new(Vec::new()));
    let seeds: SeedSource = {
        let calls = calls.clone();
        let samples = samples.clone();
        let globals = globals.clone();
        Arc::new(move || {
            samples
                .lock()
                .unwrap()
                .push(globals.lock().snapshot.tips.clone());
            calls.fetch_add(1, Ordering::SeqCst)
        })
    };
    assert!(
        shin_tasks::block_on(Scene::load_with_globals(
            server,
            &scenario,
            &saved,
            [[1, 2, 3, 4], [5, 6, 7, 8]],
            &seeds,
            globals.clone(),
            &AtomicBool::new(false),
        ))
        .is_err(),
        "Final resource preparation failure must precede any owner clone"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 100);
    assert!(samples.lock().unwrap().is_empty());
    assert_eq!(globals.lock().snapshot.tips, vec![0; 20]);

    saved.records[31][0] = 11;
    saved.records[31][6] = 7;
    let scene = shin_tasks::block_on(Scene::load_with_globals(
        server,
        &scenario,
        &saved,
        [[1, 2, 3, 4], [5, 6, 7, 8]],
        &seeds,
        globals.clone(),
        &AtomicBool::new(false),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(scene.layers.len(), 32);
    assert_eq!(calls.load(Ordering::SeqCst), 228);
    let samples = samples.lock().unwrap();
    assert_eq!(samples.len(), 128);
    let mut expected = vec![0; 20];
    for (i, (slot, id, layer)) in scene.layers.iter().enumerate() {
        assert_eq!((*slot, *id), (i as u8, (31 - i) as i32));
        for sample in &samples[4 * i..4 * i + 4] {
            assert_eq!(
                sample, &expected,
                "physicalslot{i} four seeds precede its clone acquisition"
            );
        }
        if let Layer::Temporary {
            view: temporary::View::Tips(view),
            ..
        } = layer
        {
            let id = view.rows[0].descriptor.source_index;
            assert_eq!(view.rows[0].previously_acquired, acquired(&expected, id));
            set(&mut expected, id);
        }
    }
    assert_eq!(globals.lock().snapshot.tips, expected);
    drop(samples);
    let original = native_group(&scene);
    let visits = RefCell::new(Vec::new());
    let spies = scene
        .layers
        .iter()
        .map(|(slot, _, layer)| Spy {
            slot: *slot,
            layer,
            visits: &visits,
        })
        .collect::<Vec<_>>();
    reference_render_to_texture(device, queue, (228, 128), |ctx, view| {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view,
            },
            Some(ctx.depth_stencil),
            "Mixed actual retained owners/native traversal check",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        group::render_drawables(
            &mut pass,
            spies
                .iter()
                .zip(&scene.layers)
                .map(|(spy, (_, id, _))| group::DrawableChild {
                    slot: spy.slot,
                    owner_id: *id,
                    layer: spy,
                })
                .collect(),
            &scene.screen,
            &scene
                .root
                .composed_transform(&HigurashiTransform::default()),
        )
        .unwrap();
    });
    assert_eq!(
        *visits.borrow(),
        original,
        "64 original6e340/686d8 visits; child packet service remains a checked native spy"
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicU32::new(0));
    let seeds: SeedSource = {
        let cancelled = cancelled.clone();
        let calls = calls.clone();
        Arc::new(move || {
            cancelled.store(true, Ordering::Release);
            calls.fetch_add(1, Ordering::SeqCst)
        })
    };
    let globals = globals::State::shared(globals::Snapshot::new(&settings()));
    assert!(
        shin_tasks::block_on(Scene::load_with_globals(
            server,
            &scenario,
            &saved,
            [[1, 2, 3, 4], [5, 6, 7, 8]],
            &seeds,
            globals.clone(),
            &cancelled
        ))
        .unwrap()
        .is_none()
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        4,
        "Current Null clone completes its four seeds; no following temporary clone"
    );
    assert_eq!(globals.lock().snapshot.tips, vec![0; 20]);
}

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    changed: Condvar,
}
fn verify_discarded_work(server: Arc<AssetServer>) {
    let scenario = scenario(&program(12, &[0, 0, 17, 0, 17, 244], true));
    let mut saved = settings();
    let mut slots = Slots::loaded(&saved);
    slots.manual[0].valid = true;
    slots.manual[0].data = data(0, 1, 999, 42);
    let source = slots.manual[0].data.clone();
    let calls = Arc::new(AtomicU32::new(1));
    let gate = Arc::new(Gate::default());
    let seeds: SeedSource = {
        let calls = calls.clone();
        let gate = gate.clone();
        Arc::new(move || {
            let seed = calls.fetch_add(1, Ordering::SeqCst);
            if seed == 17 {
                gate.entered.store(true, Ordering::Release);
                let mut released = gate.released.lock().unwrap();
                while !*released {
                    let (next, timeout) = gate
                        .changed
                        .wait_timeout(released, Duration::from_secs(30))
                        .unwrap();
                    released = next;
                    if timeout.timed_out() {
                        break;
                    }
                }
            }
            seed
        })
    };
    let mut service = PreviewService::new(server, scenario, (1280, 720), false, 0, seeds)
        .with_globals(globals::Snapshot::new(&saved));
    let state = state();
    until(
        || {
            service.poll_rows(&slots, &state).unwrap();
            service.capture.is_some()
        },
        "Discarded temporary capture start",
    );
    until(
        || gate.entered.load(Ordering::Acquire),
        "First Null clone barrier",
    );
    let retirement = service.capture.as_ref().unwrap().job.retirement.clone();
    service.invalidate(0);
    let index = service.capture.as_ref().unwrap().index;
    let cancelled = service
        .capture
        .as_ref()
        .unwrap()
        .cancelled
        .load(Ordering::Acquire);
    *gate.released.lock().unwrap() = true;
    gate.changed.notify_all();
    assert_eq!(index, None);
    assert!(!cancelled, "Index loss is not worker cancellation");
    until(
        || retirement.is_finished(),
        "Discarded temporary clone completion",
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        25,
        "Sixteen constructor seeds then Null/Tips four-seed clones"
    );
    assert!(service.thumbnails[0].is_none());
    assert!(
        service.capture.as_ref().unwrap().ready.is_none(),
        "Unpolled discarded worker result has no GPU publication"
    );
    let frozen = service
        .with_settings(&mut saved, |settings| Ok(settings.clone()))
        .unwrap();
    for id in [0, 17, 244] {
        assert!(acquired(&frozen.save_vectors.extra_words_0, id));
    }
    assert_eq!(frozen.save_vectors.extra_words_0.len(), 20);
    assert_eq!(slots.manual[0].data, source);
    service.retire_jobs();
    assert!(retirement.is_finished());
    assert!(service.capture.is_none() && service.hydration.is_none());
    let after = service
        .with_settings(&mut saved, |settings| Ok(settings.clone()))
        .unwrap();
    assert_eq!(
        frozen.save_vectors.extra_words_0, after.save_vectors.extra_words_0,
        "Retirement preserves clone effects before disk snapshots"
    );
}

#[test]
#[ignore = "actual retained temporary Scene/worker/capture/publication/clone effects with real assets/native packets and group traversal; pure offscreen GPU, no audio/windows/user saves"]
fn higurashi_saved_preview_temporary_scene_service_and_discarded_bits_match_native() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    verify_all_ready_order_and_cancellation(&device, &queue, &server);
    let mut frames = 0;
    let mut packets = 0;
    for canvas in [(1280, 720), (1920, 1080)] {
        for kind in [11, 12] {
            let ids = if kind == 12 {
                vec![0, 0, 17, 0, 17, 244]
            } else {
                vec![]
            };
            let scenario = scenario(&program(kind, &ids, false));
            let mut saved = settings();
            let mut slots = Slots::loaded(&saved);
            slots.manual[0].valid = true;
            slots.manual[0].data = data(0, 1, 999, 42);
            let source = slots.manual[0].data.clone();
            let calls = Arc::new(AtomicU32::new(1));
            let seeds: SeedSource = {
                let calls = calls.clone();
                Arc::new(move || calls.fetch_add(1, Ordering::SeqCst))
            };
            let mut service =
                PreviewService::new(server.clone(), scenario, canvas, false, 0, seeds)
                    .with_globals(globals::Snapshot::new(&saved));
            let state = state();
            service.poll_rows(&slots, &state).unwrap();
            assert_eq!(service.rows[0].labels, Labels::Loading);
            wait_ready(&mut service, &slots, &state);
            assert!(
                matches!(&service.rows[0].labels,Labels::Ready {title,subtitle} if title.as_ref()==""&&subtitle.as_ref()==""),
                "SAVEINFO remains unpublished without AUTOSAVE"
            );
            assert!(service.thumbnail_sources()[0].is_none());
            assert_eq!(
                calls.load(Ordering::SeqCst),
                21,
                "Sixteen constructor seeds plus one four-seed temporary owner"
            );
            let mut native = Reference::load(
                &server,
                kind,
                if kind == 11 { 7 } else { 0 },
                &ids,
                false,
                0,
            );
            packets += native.packet_count();
            let thumb = resources::thumbnail_size(canvas);
            assert_eq!(
                thumb,
                if canvas.0 == 1280 {
                    (228, 128)
                } else {
                    (342, 192)
                }
            );
            let scene = service.capture.as_ref().unwrap().ready.as_ref().unwrap();
            let actual = actual_thumbnail(&device, &queue, thumb, scene);
            let expected = original_thumbnail(&device, &queue, thumb, &native);
            assert_eq!(
                actual, expected,
                "Original temporary pixels through actual Scene::load_with_globals at{thumb:?}"
            );
            assert!(actual.chunks_exact(4).any(|v| v[..3] != [0, 0, 0]));
            frames += 1;
            if kind == 11 {
                saved.settings.common.v14_showroutenavi = true;
                service.synchronize(&mut saved).unwrap();
                native = Reference::load(&server, kind, 7, &ids, true, 0);
                packets += native.packet_count();
                let scene = service.capture.as_ref().unwrap().ready.as_ref().unwrap();
                let changed = actual_thumbnail(&device, &queue, thumb, scene);
                let expected = original_thumbnail(&device, &queue, thumb, &native);
                assert_eq!(
                    changed, expected,
                    "Live route preference affects the same retained Scene before GPU capture"
                );
                assert_ne!(actual, changed);
                assert_eq!(calls.load(Ordering::SeqCst), 21);
                frames += 1;
            } else {
                let frozen = service
                    .with_settings(&mut saved, |settings| Ok(settings.clone()))
                    .unwrap();
                for id in [0, 17, 244] {
                    assert!(acquired(&frozen.save_vectors.extra_words_0, id));
                }
                assert_eq!(frozen.save_vectors.extra_words_0.len(), 20);
                assert!(
                    frozen
                        .save_vectors
                        .extra_words_0
                        .iter()
                        .all(|v| v & 0xaaaaaaaa == 0),
                    "TIPS read bits are unchanged"
                );
            }
            let actual = reference_render_to_texture(&device, &queue, canvas, |ctx, view| {
                service.prepare(ctx, &state).unwrap();
                let texture = service.thumbnails[0].as_ref().unwrap();
                assert_eq!(texture.dimensions(), thumb);
                present(ctx, view, texture);
            });
            let expected = reference_render_to_texture(&device, &queue, canvas, |ctx, view| {
                let texture = native_capture(ctx, canvas, &native);
                assert_eq!(texture.dimensions(), thumb);
                present(ctx, view, &texture);
            });
            assert_eq!(
                actual, expected,
                "Actual PreviewService generated/published original pixels at{canvas:?}"
            );
            frames += 1;
            assert_eq!(slots.manual[0].data, source);
            service.retire_jobs();
            service.synchronize(&mut saved).unwrap();
        }
    }
    verify_discarded_work(server);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(frames, 10);
    eprintln!(
        "Saved temporary integration:{frames} byte-identical real228x128/342x192 capture and720p/1080p publication frames;{packets} original temporary reference packets;32-ready/ascending-clone/128-seed and64 original mixed-group visits;live route changes;Null-boundary cancellation;discarded index work preserves ordered TIPS acquisition before frozen settings snapshots/retirement;no audio/windows/user saves"
    );
}
