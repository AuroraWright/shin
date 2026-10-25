use super::transitions::{Frames, advance, assets, new_backend};
use super::*;
use crate::adv::gerokasu2::{
    layer_control::{LayerControlPlan, LayerControlServices, PropertyTarget},
    runner::FramePolicy,
    scene::SceneServices,
};
use shin_core::primitives::update::FrameId;

fn control(
    id: i32,
    property: LayerProperty,
    value: i32,
    duration: i32,
    flags: i32,
) -> Gerokasu2RuntimeCommand {
    use enum_map::Enum;
    Gerokasu2RuntimeCommand::LayerCtrl {
        layer_id: id,
        property_id: property.into_usize() as i32,
        params: (value, duration, flags, 0, 0, 0, 0, 0),
    }
}
fn apply(
    backend: &mut BasicLoadBackend,
    layers: &mut Gerokasu2Layers,
    command: Gerokasu2RuntimeCommand,
) {
    let mut prepared = layers.clone();
    let plan = LayerControlPlan::prepare(&mut prepared, &command).unwrap();
    plan.apply(backend).unwrap();
    *layers = prepared;
}
fn translated_green(pixels: &[u8], black_columns: usize) {
    for x in 0..320 {
        let at = (90 * 320 + x) * 4;
        let expected = if x < black_columns {
            [0, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        };
        assert_eq!(
            &pixels[at..at + 4],
            &expected,
            "translated column{x},expected black width{black_columns}"
        );
    }
}

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn actual_gerokasu2_control_properties_page_copies_and_fresh_policy() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes().into()).unwrap());
    let mut backend = new_backend(&assets, &scenario);
    let mut layers = Gerokasu2Layers::default();
    let mut frame = FrameId::default();
    let mut frames = Frames::new(&gpu);
    complete(
        &mut load(
            &mut backend,
            &mut layers,
            command(1, 0, [0xf0f0, -960, -540, 1920, 1080, 0, 0, 0]),
        ),
        &mut backend,
    );
    apply(
        &mut backend,
        &mut layers,
        control(3, LayerProperty::TranslateX, 100, 10, 0),
    );
    apply(
        &mut backend,
        &mut layers,
        control(3, LayerProperty::TranslateX, 50, 10, 128),
    );
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 5.0, true);
    let bank = layers.target(LayerId::new(3)).unwrap().layerbank;
    let plane = PlaneId::new(0);
    assert_eq!(
        backend
            .layer(plane, bank)
            .unwrap()
            .properties()
            .get_value(LayerProperty::TranslateX),
        50.0
    );
    translated_green(&frames.draw_scene(&gpu, &mut backend), 8);
    backend.pageback(false).unwrap();
    layers.begin_back();
    apply(
        &mut backend,
        &mut layers,
        control(3, LayerProperty::TranslateX, 25, 0, 128 | 512),
    );
    assert_eq!(
        backend
            .layer(plane, bank)
            .unwrap()
            .properties()
            .get_value(LayerProperty::TranslateX),
        175.0
    );
    let active = backend
        .screen()
        .reference_pages()
        .next()
        .unwrap()
        .get_plane(plane)
        .get_layer(bank)
        .unwrap();
    assert_eq!(
        active.properties().get_value(LayerProperty::TranslateX),
        50.0
    );
    drop(active);
    translated_green(&frames.draw_scene(&gpu, &mut backend), 8);
    frame.advance();
    advance(&mut backend, &gpu, &assets, frame, 5.0, true);
    translated_green(&frames.draw_scene(&gpu, &mut backend), 17);
    backend.promote_pending_page(false).unwrap();
    translated_green(&frames.draw_scene(&gpu, &mut backend), 29);
    backend.pageback(true).unwrap();
    for id in [-3, -4] {
        apply(
            &mut backend,
            &mut layers,
            control(id, LayerProperty::TranslateX, i32::MAX, 0, 0),
        );
        apply(
            &mut backend,
            &mut layers,
            control(id, LayerProperty::TranslateX, 1, 0, 0),
        );
        backend
            .visit_properties(
                &[if id == -3 {
                    PropertyTarget::Page
                } else {
                    PropertyTarget::Plane(plane)
                }],
                &mut |props| {
                    assert_eq!(props.get_value(LayerProperty::TranslateX), 0.0);
                    assert_eq!(
                        props
                            .property_tweener(LayerProperty::TranslateX)
                            .target_value(),
                        1.0
                    );
                },
            )
            .unwrap();
        crate::adv::gerokasu2::layer_control::initialize(&mut layers, id, &mut backend).unwrap();
    }
    assert!(!backend.supports_root());
    let mut called = false;
    assert!(
        backend
            .visit_properties(&[PropertyTarget::Screen, PropertyTarget::Root], &mut |_| {
                called = true
            })
            .is_err()
    );
    assert!(!called);
    backend.promote_pending_page(false).unwrap();
    assert!(
        frames
            .draw_scene(&gpu, &mut backend)
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255])
    );
    println!(
        "GEROKASU2_CONTROL_GPU copied_pending_queues_active_render_fresh_page_policy_atomic_validation=verified frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}

struct Services {
    loader: BasicLoadBackend,
    assets: Arc<AssetServer>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    started: Instant,
    fail_next: bool,
    injected: bool,
}
impl LayerControlServices for Services {
    fn visit_properties(
        &mut self,
        targets: &[PropertyTarget],
        action: &mut dyn FnMut(&mut LayerProperties),
    ) -> Result<()> {
        if self.fail_next {
            self.fail_next = false;
            anyhow::bail!("injected control owner error");
        }
        self.loader.visit_properties(targets, action)
    }
    fn bank_wiping(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<bool> {
        self.loader.bank_wiping(plane, bank)
    }
}
impl MessageNotifications for Services {
    fn observed_voice(&mut self, _: &str) {}
    fn completed(&mut self, _: u32) {}
}
impl RunnerServices for Services {
    fn layer_load_services(&mut self) -> Option<&mut (dyn LoadServices + '_)> {
        Some(&mut self.loader)
    }
    fn scene_services(&mut self) -> Option<&mut (dyn SceneServices + '_)> {
        Some(&mut self.loader)
    }
    fn layer_control_services(&mut self) -> Option<&mut (dyn LayerControlServices + '_)> {
        Some(self)
    }
    fn command_context(
        &mut self,
        c: &Gerokasu2RuntimeCommand,
        first: bool,
    ) -> Result<MessageCommandContext> {
        self.loader.set_frame(self.started, false);
        if first
            && !self.injected
            && matches!(c, Gerokasu2RuntimeCommand::LayerCtrl { layer_id: -6, .. })
        {
            self.injected = true;
            self.fail_next = true;
        }
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
        unreachable!()
    }
    fn advance_layers_in_frame(
        &mut self,
        frame: FrameId,
        _: &Gerokasu2Layers,
        delta: Ticks,
        allowed: bool,
    ) -> Result<()> {
        self.loader.update(&crate::update::AdvUpdateContext {
            frame_id: frame,
            delta_ticks: delta,
            asset_server: &self.assets,
            device: &self.device,
            queue: &self.queue,
            are_animations_allowed: allowed,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        Ok(())
    }
    fn fast_forward_layers(&mut self, _: &Gerokasu2Layers) -> Result<()> {
        self.loader.fast_forward();
        Ok(())
    }
}
fn policy(paused: bool) -> FramePolicy {
    FramePolicy {
        paused,
        scene_changing: false,
        animations_allowed: true,
        autoplay: false,
        fast_skip: false,
    }
}

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output"]
fn decoded_gerokasu2_controls_retained_waits_retry_transwait_and_exit() {
    let gpu = Offscreen::new();
    let assets = assets(&gpu);
    let source = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(source.header.code_offset as usize);
    fn num(data: &mut Vec<u8>, v: i32) {
        let mut out = Cursor::new(Vec::new());
        NumberSpec::<i32>::constant(v).write_le(&mut out).unwrap();
        data.extend(out.into_inner());
    }
    fn op(data: &mut Vec<u8>, code: u8, args: &[i32]) {
        data.push(code);
        for &v in args {
            num(data, v);
        }
    }
    fn layer(data: &mut Vec<u8>, id: i32, color: i32, flags: i32) {
        op(data, 0xc1, &[id, 1, flags]);
        data.push(31);
        for v in [color, -960, -540, 1920, 1080] {
            num(data, v);
        }
    }
    fn ctrl(data: &mut Vec<u8>, id: i32, p: i32, value: i32, duration: i32, flags: i32) {
        op(data, 0xc3, &[id, p]);
        data.push(7);
        for v in [value, duration, flags] {
            num(data, v);
        }
    }
    fn wait(data: &mut Vec<u8>, id: i32, p: i32) {
        op(data, 0xc4, &[id]);
        data.push(1);
        num(data, p);
    }
    layer(&mut data, 3, 0xff00, 0);
    layer(&mut data, 5, 0xf0f0, 0);
    op(&mut data, 0xc6, &[3, 5]);
    ctrl(&mut data, -6, 0, 96, 30, 0);
    ctrl(&mut data, -6, 0, 192, 30, 128);
    wait(&mut data, -6, 0);
    op(&mut data, 0xc0, &[-6]);
    op(&mut data, 0xcc, &[1]);
    layer(&mut data, 3, 0xf00f, 0);
    ctrl(&mut data, -4, 0, 96, 30, 0);
    wait(&mut data, -4, 0);
    op(&mut data, 0xc0, &[-4]);
    op(&mut data, 0xcc, &[0]);
    ctrl(&mut data, -2, 9, 500, 30, 0);
    wait(&mut data, -2, 9);
    op(&mut data, 0xc0, &[-2]);
    ctrl(&mut data, -3, 0, 96, 30, 0);
    wait(&mut data, -3, 0);
    op(&mut data, 0xc0, &[-3]);
    op(&mut data, 0xc9, &[0, 2, 30]);
    data.push(0);
    layer(&mut data, 3, 0xf00f, 4);
    op(&mut data, 0xca, &[-6]);
    data.extend([0, 0]);
    num(&mut data, 0);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Arc::new(Gerokasu2Scenario::new(data.into()).unwrap());
    let mut services = Services {
        loader: new_backend(&assets, &scenario),
        assets: assets.clone(),
        device: gpu.device.clone(),
        queue: gpu.queue.clone(),
        started: Instant::now(),
        fail_next: false,
        injected: false,
    };
    let mut runner = shin_tasks::block_on(Gerokasu2Runner::load(
        assets,
        scenario,
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
    let mut frames = Frames::new(&gpu);
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut retry = false;
    let mut motion = 0;
    let mut waits = Vec::new();
    let mut trans = false;
    let mut paused = false;
    loop {
        services.started = Instant::now();
        let status = match runner.pump(false, 100, 10000, &mut services) {
            Ok(s) => s,
            Err(e) => {
                assert!(e.to_string().contains("injected control owner error"));
                assert!(!retry);
                retry = true;
                let bank = runner.layers().target(LayerId::new(3)).unwrap().layerbank;
                assert_eq!(
                    runner
                        .layers()
                        .record(bank)
                        .property(LayerProperty::TranslateX),
                    0
                );
                assert_eq!(
                    services
                        .loader
                        .layer(PlaneId::new(0), bank)
                        .unwrap()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    0.0
                );
                continue;
            }
        };
        if status == RunnerStatus::Exited {
            break;
        }
        assert_eq!(status, RunnerStatus::Waiting);
        assert!(Instant::now() < deadline);
        let waiting = match runner.pending_command() {
            Some(Gerokasu2RuntimeCommand::LayerWait { layer_id, .. }) => Some(*layer_id),
            _ => None,
        };
        if let Some(id) = waiting {
            if !waits.contains(&id) {
                waits.push(id);
            }
        }
        if matches!(
            runner.pending_command(),
            Some(Gerokasu2RuntimeCommand::TransWait { .. })
        ) {
            trans = true;
        }
        if waiting == Some(-6) && !paused {
            paused = true;
            let bank = runner.layers().target(LayerId::new(3)).unwrap().layerbank;
            for stop in [true, false] {
                frame.advance();
                services.loader.set_animations_allowed(stop);
                runner
                    .advance(frame, Ticks::from_f32(15.0), policy(stop), &mut services)
                    .unwrap();
                assert_eq!(
                    services
                        .loader
                        .layer(PlaneId::new(0), bank)
                        .unwrap()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    0.0
                );
            }
            services.loader.set_animations_allowed(true);
        }
        frame.advance();
        runner
            .advance(frame, Ticks::from_f32(15.0), policy(false), &mut services)
            .unwrap();
        let pixels = frames.draw_scene(&gpu, &mut services.loader);
        if waiting == Some(-6) {
            motion += 1;
            let x = [48.0, 96.0, 192.0, 288.0][motion - 1];
            for id in [3, 5] {
                let bank = runner.layers().target(LayerId::new(id)).unwrap().layerbank;
                assert_eq!(
                    services
                        .loader
                        .layer(PlaneId::new(0), bank)
                        .unwrap()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    x
                );
            }
            translated_green(&pixels, (x / 6.0) as usize);
        }
    }
    assert!(retry && paused && trans);
    assert_eq!(motion, 4);
    assert_eq!(waits, [-6, -4, -2, -3]);
    assert_eq!(runner.completed_commands(), 23);
    assert_eq!(services.loader.loaded_notifications().len(), 4);
    assert!(
        frames
            .draw_scene(&gpu, &mut services.loader)
            .chunks_exact(4)
            .all(|p| p == [0, 0, 255, 255])
    );
    println!(
        "GEROKASU2_CONTROL_RUNNER commands=23 motion_frames=4 wait_owners=4 retry_once_pause_animation_gate_TRANSWAIT_EXIT=verified frames={} channels={} audio=default output",
        frames.frames, frames.channels
    );
}
