use super::*;
use super::last_render::{LastPropertyTest, render_captured_draw_packets};

// Executed original/1.03 draw packets are the expected geometry, colors, UVs,
// transforms and stencil state. The actual path uses the complete layer wrapper.
use crate::layer::{DrawableLayer, Layer, LayerProperties};
use crate::update::AdvUpdatable;
use shin_core::{primitives::update::FrameId, time::{Ticks, Tween},
};

#[derive(Clone)]
struct PlainDrawCase {
    name: String,
    cake: bool,
    mask: i32,
    commands: Vec<String>,
    properties: LastPropertyTest,
    parent: TransformParams,
    passes: u8,
    stencil: u8,
    prepare_properties: bool,
}
impl PlainDrawCase {
    fn defaults(cake: bool, name: &str) -> Self {
        use num_traits::FromPrimitive;
        Self {
            name: name.into(), cake, mask: 127, commands: Vec::new(),
            properties: LastPropertyTest {
                values: std::array::from_fn(|i| {
                    shin_core::vm::command::types::LayerProperty::from_usize(i).unwrap().initial_value() as f32
                }),
                wobblers: [(0xfffffff0, 0, 0.0), (23, 0, 0.0), (17, 0, 0.0), (29, 0, 0.0), (0x80000001, 0, 0.0), (31, 0, 0.0),
                ],
                advance: None, queued: None, fast_forward: false,
            },
            parent: TransformParams::default(), passes: 3, stencil: 0, prepare_properties: true,
        }
    }
    fn native_request(&self) -> String {
        let mut request = if self.cake { "N\n".into() } else { format!("N {}\n", self.mask) };
        request += "A";
        for value in self.parent.transform.to_cols_array().into_iter()
            .chain(self.parent.camera_position.to_array())
            .chain(self.parent.unconditionally_inherited_translation.to_array())
            .chain(self.parent.wobble_translation.to_array()) {
            request += &format!(" {:08x}", value.to_bits());
        }
        request += "\nE";
        for value in self.properties.values { request += &format!(" {:08x}", value.to_bits()); }
        for (seed, mode, time) in self.properties.wobblers {
            request += &format!(" {seed:08x} {:08x} {:08x}", mode as u32, time.to_bits());
        }
        request += &format!(" {} {:08x}\n", u8::from(self.properties.advance.is_some()), self.properties.advance.unwrap_or(0.0).to_bits());
        if let Some((index, target, duration)) = self.properties.queued {
            request += &format!("Y {index} {:08x} {duration}\n", target.to_bits());
        }
        request += &(self.commands.join("\n") + "\n");
        if self.properties.fast_forward { request += "F\n"; }
        request += &format!("S\nV {}\n", self.passes);
        request
    }
}

enum PlainTestLayer {
    Quiz(QuizLayer),
    Cake(crate::layer::user::cake_layer::CakeLayer),
}
macro_rules! with_plain_layer {
    ($layer:expr, $view:ident, $body:expr) => {
        match $layer { PlainTestLayer::Quiz($view) => $body, PlainTestLayer::Cake($view) => $body,
        }
    };
}
impl PlainTestLayer {
    fn properties(&self) -> &LayerProperties { with_plain_layer!(self, view, view.properties()) }
    fn properties_mut(&mut self) -> &mut LayerProperties { with_plain_layer!(self, view, view.properties_mut()) }
    fn command(&self, command: &str) {
        if let Self::Cake(view) = self { view.reference_draw_command(command); return; }
        let Self::Quiz(view) = self else { unreachable!() };
        let mut model = view.inner_ref().model.lock().unwrap();
        let mut parts = command.split_whitespace();
        let op = parts.next().unwrap();
        let args: Vec<f32> = parts.map(|p| p.parse().unwrap()).collect();
        use crate::adv::input_area::InputAreaTarget;
        match op {
            "C" => model.controller_input(args[0] != 0.0, args[1] != 0.0, args[2] != 0.0),
            "P" => { assert!(model.probe(Vec2::new(args[0], args[1]))); }
            "H" => model.press(Vec2::new(args[0], args[1])),
            "R" => model.release(Vec2::new(args[0], args[1])),
            _ => panic!("Unsupported native Quiz command {command}"),
        }
    }
}

struct PlainDrawHost {
    device: wgpu::Device,
    queue: wgpu::Queue,
    server: Arc<crate::asset::system::AssetServer>,
    samplers: shin_render::shaders::types::texture::TextureSamplerStore,
    pipelines: shin_render::pipelines::PipelineStorage,
    dynamic: shin_render::dynamic_buffer::DynamicBuffer,
}
impl PlainDrawHost {
    fn new() -> Self {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_render::shaders::types::{buffer::BytesAddress, texture::TextureSamplerStore};
        let instance = wgpu::Instance::default();
        let adapter = shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
        let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
                } else {
                    wgpu::Features::empty()
                },
            ..Default::default()
        })).unwrap();
        let mut io = LayeredAssetIo::new();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") { io.try_with_rom(patch).unwrap(); }
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap();
        let server = Arc::new(AssetServer::new(io.into(), AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device.clone(), wgpu_queue: queue.clone(), bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
        ));
        Self {
            samplers: TextureSamplerStore::new(&device),
            pipelines: shin_render::pipelines::PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm,
            ),
            dynamic: shin_render::dynamic_buffer::DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024),
            ),
            device, queue, server,
        }
    }
    fn render(&mut self, case: &PlainDrawCase, captured: Option<&str>) -> Vec<u8> {
        use crate::render::PreRenderContext;
        use shin_render::{resize::{SurfaceResizeSource, ViewportParams}, shaders::types::texture::{DepthStencilTarget, TextureTarget, TextureTargetKind},
        };
        let mut layer = if case.cake {
            PlainTestLayer::Cake(shin_tasks::block_on(crate::layer::user::cake_layer::CakeLayer::load(&self.server),
            ))
        } else {
            PlainTestLayer::Quiz(shin_tasks::block_on(QuizLayer::load(&self.server, case.mask,
            )))
        };
        if case.prepare_properties {
            layer.properties_mut().reference_install_native_state(&case.properties.values, &case.properties.wobblers, case.properties.advance,
            );
            if let Some((index, target, duration)) = case.properties.queued {
                use num_traits::FromPrimitive;
                layer.properties_mut().property_tweener_mut(shin_core::vm::command::types::LayerProperty::from_usize(index).unwrap(),
                    )
                    .enqueue(target, Tween::linear(Ticks::from_f32(duration)));
            }
        } else { assert!(layer.properties().initialized_identity().is_none()); }
        let mut frame_id = FrameId::default();
        for command in &case.commands {
            if let Some(delta) = command.strip_prefix("T ") {
                frame_id.advance();
                let context = crate::update::AdvUpdateContext {
                    frame_id, delta_ticks: Ticks::from_f32(delta.parse().unwrap()),
                    asset_server: &self.server, device: &self.device, queue: &self.queue,
                    are_animations_allowed: true, voice_callback: None, movie_volume_settings: Default::default(),
                };
                with_plain_layer!(&mut layer, view, view.update(&context));
            } else { layer.command(command); }
        }
        if case.properties.fast_forward { with_plain_layer!(&mut layer, view, view.fast_forward()); }
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(1920, 1080,
        )));
        let create_texture = |label, format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label), size: wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1,
                },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format, usage, view_formats: &[],
        })
        };
        let target = create_texture("Plain drawable target", wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = create_texture("Plain drawable stencil", shin_render::DEPTH_STENCIL_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let target_view = target.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            breakup_texture: None, device: &self.device, queue: &self.queue, resize_source: &resize,
            sampler_store: &self.samplers, depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut self.pipelines, dynamic_buffer: &mut self.dynamic, encoder: &mut encoder,
        };
        if captured.is_none() { with_plain_layer!(&mut layer, view, view.pre_render(&mut context, &case.parent)); }
        {
            let mut pass = context.begin_pass(TextureTarget { view: &target_view, kind: TextureTargetKind::RenderTexture,
                }, Some(DepthStencilTarget { view: &depth_view }), "Plain native drawable comparison",
            );
            pass.clear(Some(shin_core::primitives::color::UnormColor::BLACK), Some(case.stencil), Some(1.0),
            );
            for (bit, kind) in [(1, PassKind::Opaque), (2, PassKind::Transparent)] {
                if case.passes & bit == 0 { continue; }
                if let Some(captured) = captured {
                    match &layer {
                        PlainTestLayer::Quiz(view) => render_captured_draw_packets(|name| match name {
                            "button" => &view.inner_ref().textures.button,
                            "consult" => &view.inner_ref().textures.consult,
                            _ => panic!("Native Quiz texture {name}"),
                        }, &mut pass, captured, kind,
                        ),
                        PlainTestLayer::Cake(view) => render_captured_draw_packets(|name| view.inner_ref().reference_texture(name), &mut pass, captured, kind,
                        ),
                    }
                } else { with_plain_layer!(&layer, view, view.render(&mut pass, &case.parent, 4, kind)); }
            }
        }
        drop(context);
        if !case.prepare_properties { assert!(layer.properties().initialized_identity().is_none(), "{}: opaque/pre-render must not materialize properties", case.name); }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None, size: 1920 * 1080 * 4, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(target.as_image_copy(), wgpu::TexelCopyBufferInfo {
            buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1920 * 4), rows_per_image: Some(1080),
                },
        }, target.size(),
        );
        let mut uploads = self.device.create_command_encoder(&Default::default());
        self.dynamic.finish(&mut uploads);
        self.queue.submit([uploads.finish(), encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| send.send(r).unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement"); receive.recv().unwrap().unwrap();
        readback.slice(..).get_mapped_range()
            .expect("Readback mapping must be ready").to_vec()
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires supplied Quiz/Cake executables/atlases, Metal and the default audio output"]
fn reference_plain_quiz_cake_native_draws_and_properties() {
    use std::{io::Write, process::{Command, Stdio},
    };
    crate::init_test_task_pools();
    let _audio = shin_audio::AudioManager::new_for_tests();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let executable = root.join("game-data/umineko").join(if updated { "uncompressed_executable_update" } else { "uncompressed_executable" });
    let mut host = PlainDrawHost::new();
    let mut cases = Vec::new();
    for mask in 1..=127 {
        let mut case = PlainDrawCase::defaults(false, &format!("mask{mask:03}")); case.mask = mask; cases.push(case);
    }
    // Use the independently read native hit map to choose an interior point of
    // every authored slice; do not generate inputs through the port hit test.
    let raw = std::fs::read(&executable).unwrap();
    let offset = 256 + if updated { 0x1675e9 } else { 0x166529 };
    let map = &raw[offset..offset + 28800];
    let positions: [Vec2; 15] = std::array::from_fn(|row| {
        let pixel = (0..57600).find(|&i| {
            let nibble = if i % 2 == 0 { map[i / 2] >> 4 } else { map[i / 2] & 15 };
            nibble as usize == row + 1
        }).unwrap();
        Vec2::new(482.0 + (pixel % 240) as f32 * 4.0, 2.0 + (pixel / 240) as f32 * 4.0,
        )
    });
    for (row, position) in positions.iter().enumerate() {
        let mut case = PlainDrawCase::defaults(true, &format!("slice{row:02}"));
        case.commands = vec![format!("P {} {}", position.x, position.y), format!("H {} {}", position.x, position.y), "T 1".into(),
        ];
        cases.push(case);
    }
    let configs: &[(&str, &[(usize, f32)])] = &[
        ("properties-tint", &[(6, 250.0), (7, 1700.0), (8, 500.0)]),
        ("properties-hidden-alpha", &[(22, 0.0), (9, 0.0), (40, 1.0), (41, 10.0), (42, 600.0), (43, 200.0),
            ],
        ),
        ("properties-blend", &[(23, 2.0)]),
        ("properties-fragment-clip", &[(24, 5.0), (61, 1.0), (62, -150.0), (63, -100.0), (64, 200.0), (65, 130.0),
            ],
        ),
        ("properties-effects", &[(56, 800.0), (66, 10000.0), (67, 64.0), (68, 500.0), (76, 100.0), (82, 100.0),
            ],
        ),
        ("properties-scale-origin", &[(0, 81.25), (1, -62.75), (10, 133.5), (11, -75.25), (12, 750.0), (13, 1200.0), (14, 1100.0), (15, 900.0),
            ],
        ),
        ("properties-rotation-origin", &[(16, 41.75), (17, -20.5), (18, 25.0), (19, -11.0), (20, 17.5), (21, -9.25),
            ],
        ),
        ("properties-flip", &[(26, 3.0), (3, 48.75), (4, -33.5)]),
        ("properties-camera", &[(2, 1500.0), (85, 500.0), (86, -800.0), (87, 2000.0), (88, -700.0), (89, 400.0),
            ],
        ),
        ("properties-wobblers", &[(32, 6.0), (33, 17.0), (34, 90.0), (36, 1.0), (37, 11.0), (38, 50.0), (44, 3.0), (45, 10.0), (46, 15.0), (48, 1.0), (49, 12.0), (50, 120.0), (52, 4.0), (53, 19.0), (54, 200.0),
            ],
        ),
        ("properties-frozen-clock", &[(32, 1.0), (33, 10.0), (34, 120.0)],
        ),
        ("properties-frozen-fast-forward", &[(44, 1.0), (45, 10.0), (46, 10.0)],
        ),
    ];
    for cake in [false, true] {
        for &(name, overrides) in configs {
            for phase in 0..3 {
                let mut case = PlainDrawCase::defaults(cake, &format!("{name}-phase{phase}"));
                for &(index, value) in overrides { case.properties.values[index] = value; }
                case.properties.advance = Some(2.25);
                if name.contains("frozen") { case.properties.queued = Some((0, 320.0, 100.0)); }
                case.properties.fast_forward = name.ends_with("fast-forward");
                case.parent = TransformParams {
                    transform: glam::Mat4::from_scale_rotation_translation(glam::vec3(0.85, 1.1, 1.0), glam::Quat::from_rotation_z(0.07), glam::vec3(37.0, -23.0, 0.0),
                    ),
                    camera_position: glam::vec3(-45.0, 31.0, 200.0),
                    unconditionally_inherited_translation: Vec2::new(27.0, -13.0), wobble_translation: Vec2::new(11.0, 7.0),
                };
                case.commands = if cake {
                    let p = positions[phase * 7];
                    vec![format!("P {} {}", p.x, p.y), format!("H {} {}", p.x, p.y), format!("T {}", [0.25, 1.0, 3.5][phase]),
                    ]
                } else {
                    case.mask = [1, 69, 127][phase];
                    vec!["C 0 1 0".into(), format!("T {}", [0.25, 4.0, 57.0][phase])]
                };
                cases.push(case);
            }
        }
        for (name, passes, stencil) in [("opaque-only", 1, 0), ("equal-stencil", 3, 4), ("higher-stencil", 3, 6), ("transparent-only", 2, 0),
        ] {
            let mut case = PlainDrawCase::defaults(cake, name);case.passes = passes;case.stencil = stencil;
            case.prepare_properties = passes != 1;cases.push(case);
        }
    }
    assert_eq!(cases.len(), 222);
    let mut frames = [0, 0];
    for cake in [false, true] {
        if let Ok(kind) = std::env::var("SHIN_REFERENCE_PLAIN_KIND") {
            assert!(matches!(kind.as_str(), "Cake" | "Quiz"));
            if cake != (kind == "Cake") { continue; }
        }
        let binary = std::env::temp_dir().join(format!("shin-plain-draw-{}-{cake}", std::process::id()));
        let mut compiler = Command::new("/usr/bin/clang");compiler.args(["-O0", "-Wall", "-Wextra"]);
        if cake { compiler.arg("-DSHIN_CAKE_DRAW"); }
        let compile = compiler.arg(root.join("reverse-engineering/native-plain-interactive-draw.c")).arg("-o").arg(&binary).output().unwrap();
        assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
        for case in cases.iter().filter(|c| c.cake == cake) {
            if let Ok(filter) = std::env::var("SHIN_REFERENCE_PLAIN_CASE_FILTER") { if !case.name.contains(&filter) { continue; } }
            let mut child = Command::new(&binary).arg(&executable).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(case.native_request().as_bytes()).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{}: {}", case.name, String::from_utf8_lossy(&output.stderr));
            let native = String::from_utf8(output.stdout).unwrap();
            let header: Vec<_> = native.lines().next().unwrap().split_whitespace().collect();
            assert_eq!(header[0], "V");
            let draws = header[1].parse::<usize>().unwrap();
            assert_eq!(header[2].parse::<u32>().unwrap(), u32::from(case.passes & 2 != 0), "{}: native properties are read once, only in the transparent pass", case.name);
            assert_eq!(draws, native.lines().filter(|l| l.starts_with("Q ")).count());
            let actual = host.render(case, None);
            let expected = host.render(case, Some(&native));
            let visible = expected.chunks_exact(4).filter(|p| p[..3].iter().any(|&v| v != 0)).count();
            if case.passes & 2 == 0 || case.stencil > 4 { assert_eq!(visible, 0); }
            else { assert!(visible > 1000, "{}: native artwork must be visible", case.name); }
            let worst = actual.iter().zip(&expected).map(|(a,b)|(*a as i16 - *b as i16).unsigned_abs()).max().unwrap();
            assert!(worst <= 2, "{} {}: native packet RGBA error {worst}", if cake { "Cake" } else { "Quiz" }, case.name);
            frames[usize::from(cake)] += 1;
            eprintln!("Plain {} {}: {draws} native draws,2073600 pixels xRGBA,maxByteError {worst},visiblePixels {visible}", if cake { "Cake" } else { "Quiz" }, case.name);
        }
        std::fs::remove_file(binary).unwrap();
    }
    assert!(frames.iter().sum::<usize>() > 0);
    if std::env::var_os("SHIN_REFERENCE_PLAIN_CASE_FILTER").is_none()
        && std::env::var_os("SHIN_REFERENCE_PLAIN_KIND").is_none() {
        assert_eq!(frames, [167, 55]);
    }
    eprintln!("Plain frame coverage: Quiz {},Cake {}", frames[0], frames[1]);
}
