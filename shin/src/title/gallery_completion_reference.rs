//! Native SimpleText/FontText geometry and palette; shared Metal packet replay
//! validates the desktop renderer, without claiming independent NVN output.
use crate::{
    adv::DialogText,
    asset::{
        font::GpuFontLazy,
        system::{
            Asset, AssetDataAccessor, AssetLoadContext, AssetServer, LayeredAssetIo,
            cache::AssetCache,
        },
    },
    render::PreRenderContext,
};
use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::{BytesAddress, VertexSource},
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        vertices::TextVertex,
    },
};
use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};

struct FontBytes(Vec<u8>);
impl Asset for FontBytes {
    type Args = ();
    async fn load(
        _: &Arc<AssetLoadContext>,
        _: (),
        _: &str,
        data: AssetDataAccessor,
    ) -> anyhow::Result<Self> {
        Ok(Self(data.read_all().await))
    }
}
struct Packet {
    program: u32,
    codepoint: char,
    matrix: Mat4,
    colors: [FloatColor4; 2],
    distances: [Vec2; 8],
    vertices: [TextVertex; 4],
}
fn parse(native: &str) -> Vec<Vec<Packet>> {
    let mut frames: Vec<Vec<Packet>> = Vec::new();
    for line in native.lines() {
        let w: Vec<_> = line.split_whitespace().collect();
        if w[0] == "V" {
            frames.push(Vec::new());
            continue;
        }
        assert_eq!(w[0], "Q");
        assert_eq!(w.len(), 66);
        let int = |i: usize| w[i].parse::<u32>().unwrap();
        let f = |i| f32::from_bits(int(i));
        assert_eq!(int(2), 2);
        assert_eq!(int(3), 2);
        let color = |i| FloatColor4::from_rgba(f(i), f(i + 1), f(i + 2), f(i + 3));
        frames.last_mut().unwrap().push(Packet {
            program: int(1),
            codepoint: char::from_u32(int(4)).unwrap(),
            matrix: Mat4::from_cols_array(&std::array::from_fn(|i| f(6 + i))),
            colors: [color(22), color(26)],
            distances: std::array::from_fn(|i| vec2(f(30 + 2 * i), f(31 + 2 * i))),
            vertices: std::array::from_fn(|i| TextVertex {
                position: vec4(f(46 + 5 * i), f(47 + 5 * i), f(48 + 5 * i), f(49 + 5 * i)),
                color: f(50 + 5 * i),
            }),
        });
    }
    frames
}
enum Fixture {
    Simple(DialogText),
    Cg(super::cg_gallery_text::CgCompletionText),
    Music(super::music_gallery_text::MusicCompletionText),
    Movie(super::movie_gallery_text::MovieCompletionText),
}
impl Fixture {
    fn text(&self) -> &DialogText {
        match self {
            Self::Simple(t) => t,
            Self::Cg(t) => t.reference_text(),
            Self::Music(t) => t.reference_text(),
            Self::Movie(t) => t.reference_text(),
        }
    }
    fn render(&self, pass: &mut shin_render::render_pass::RenderPass) {
        match self {
            Self::Cg(t) => t.render(pass),
            Self::Music(t) => t.render(pass),
            Self::Movie(t) => t.render(pass),
            Self::Simple(t) => t.render_with_builder(
                pass,
                crate::render::top_left_projection_matrix()
                    * Mat4::from_translation(glam::vec3(48.0, 1012.0, 0.0)),
                FloatColor4::WHITE,
                FloatColor4::from_rgba(47.0 / 255.0, 25.0 / 255.0, 20.0 / 255.0, 1.0),
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
            ),
        }
    }
}
#[test]
#[ignore = "requires supplied executables/ROM and Metal"]
fn reference_native_gallery_completion_geometry_and_pixels() {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let rom = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap());
    let mut io = LayeredAssetIo::new();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(&rom).unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let font: Arc<GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
    let folder = std::env::temp_dir().join(format!("shin-gallery-font-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let mut fonts = Vec::new();
    for name in ["newrodin-medium.fnt", "newrodin-bold.fnt", "system.fnt"] {
        let data: Arc<FontBytes> = server.load_sync(&format!("/{name}")).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, &data.0).unwrap();
        fonts.push(path);
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = folder.join("oracle");
    assert!(
        Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-gallery-font-draw.c"))
            .arg("-lm")
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let mut labels: Vec<String> = ["0.0", "0.3", "0.4", "12.7", "56.8", "99.9", "100"]
        .map(|n| format!("達成率:{n}％"))
        .into();
    let mut fixtures: Vec<_> = labels
        .iter()
        .map(|l| Fixture::Simple(DialogText::load_simple_text(&server, &font, l, 40.0, 640.0)))
        .collect();
    let scenario: Arc<shin_core::format::scenario::Scenario> =
        server.load_sync("/main.snr").unwrap();
    let info = scenario.info_tables();
    for mode in 0..3 {
        let mut save = shin_core::format::save::SaveData::default();
        let picture_ids: Vec<_> = info
            .picture_box_info
            .iter()
            .flat_map(|row| &row.picture_ids.0)
            .collect();
        for id in picture_ids
            .iter()
            .take(if mode == 2 { picture_ids.len() } else { mode })
        {
            save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
        }
        for item in info.music_box_info.iter().take(if mode == 2 {
            info.music_box_info.len()
        } else {
            mode
        }) {
            save.save_vectors
                .unlocked_bgms
                .add(item.bgm_id.into_i32() as usize);
        }
        for id in super::movie_gallery::MOVIE_GALLERY_IDS
            .into_iter()
            .take(if mode == 2 { 10 } else { mode })
        {
            save.save_vectors.unlocked_movies.add(id as usize);
        }
        let cg = super::cg_gallery::CgGalleryList::new(&info.picture_box_info, &save).unwrap();
        labels.push(cg.completion_label());
        fixtures.push(Fixture::Cg(super::cg_gallery_text::CgCompletionText::new(
            &server, &font, &cg,
        )));
        let music =
            super::music_gallery::MusicGalleryList::new(&info.music_box_info, &save).unwrap();
        labels.push(music.completion_label());
        fixtures.push(Fixture::Music(
            super::music_gallery_text::MusicCompletionText::new(&server, &font, &music),
        ));
        let movie = super::movie_gallery::MovieGalleryList::new(&info.movie_info, &save).unwrap();
        labels.push(movie.completion_label());
        fixtures.push(Fixture::Movie(
            super::movie_gallery_text::MovieCompletionText::new(&server, &font, &movie),
        ));
    }
    let codepoints: Vec<_> = labels.iter().flat_map(|s| s.chars()).collect();
    let handles = font
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &codepoints);
    let glyphs: HashMap<_, _> = codepoints.into_iter().zip(handles).collect();
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gallery font reference"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let depth = texture(
        shin_render::DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let depth_view = depth.create_view(&Default::default());
    let target = texture(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let view = target.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
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
    let input = labels
        .iter()
        .map(|s| {
            s.as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                + "\n"
        })
        .collect::<String>();
    for version in ["uncompressed_executable", "uncompressed_executable_update"] {
        let mut child = Command::new(&binary)
            .arg(rom.parent().unwrap().join(version))
            .args(&fonts)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let data = input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let trace = String::from_utf8(output.stdout).unwrap();
        std::fs::write(folder.join(format!("{version}.trace")), &trace).unwrap();
        let frames = parse(&trace);
        assert_eq!(frames.len(), labels.len());
        for ((label, text), packets) in labels.iter().zip(&fixtures).zip(&frames) {
            let local = text.text().reference_glyphs();
            assert_eq!(packets.len(), 2 * local.len());
            for (g, p) in local.iter().zip(packets) {
                let expected = vec4(
                    p.vertices[0].position.x,
                    p.vertices[0].position.y,
                    p.vertices[3].position.x - p.vertices[0].position.x,
                    p.vertices[3].position.y - p.vertices[0].position.y,
                );
                assert!(
                    (g.rect - expected).abs().max_element() < 0.0001,
                    "native glyph geometry {label} {}",
                    g.codepoint
                );
                assert_eq!(g.codepoint, p.codepoint);
                assert_eq!(p.program, 4);
                assert!(
                    (g.uv
                        - vec4(
                            p.vertices[0].position.z,
                            p.vertices[0].position.w,
                            p.vertices[3].position.z,
                            p.vertices[3].position.w
                        ))
                    .abs()
                    .max_element()
                        < 0.000001
                );
                for (a, b) in g.distances.iter().zip(&p.distances) {
                    assert!((*a - *b).abs().max_element() < 0.000001);
                }
                assert_eq!(
                    p.colors[1],
                    FloatColor4::from_rgba(47.0 / 255.0, 25.0 / 255.0, 20.0 / 255.0, 1.0)
                );
                assert_eq!(
                    p.matrix,
                    Mat4::from_translation(glam::vec3(48.0, 1012.0, 0.0))
                );
            }
            for p in &packets[local.len()..] {
                assert_eq!(p.program, 3);
                assert_eq!(p.colors, [FloatColor4::WHITE; 2]);
            }
            // Compare actual wrapper drawing to packets emitted by the unchanged
            // native builder/renderer using real font textures and shared Metal.
            for background in [UnormColor::BLACK, UnormColor::WHITE] {
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(context.depth_stencil),
                        "desktop completion text",
                    );
                    pass.clear(Some(background), None, None);
                    text.render(&mut pass);
                }
                let actual = crate::layer::user::readback_test_texture(&mut context, &target);
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(context.depth_stencil),
                        "native completion packets",
                    );
                    pass.clear(Some(background), None, None);
                    for p in packets {
                        let glyph = glyphs[&p.codepoint]
                            .as_texture_source()
                            .expect("reference glyph upload");
                        let vertices = VertexSource::VertexData {
                            vertices: &p.vertices,
                        };
                        let transform = crate::render::top_left_projection_matrix() * p.matrix;
                        let request = if p.program == 3 {
                            RenderProgramWithArguments::Font {
                                vertices,
                                glyph,
                                transform,
                                color1: p.colors[0],
                                color2: p.colors[1],
                            }
                        } else {
                            RenderProgramWithArguments::FontBorder {
                                vertices,
                                glyph,
                                transform,
                                distances: p.distances,
                                color: p.colors[1],
                            }
                        };
                        pass.run(
                            RenderRequestBuilder::new()
                                .color_blend_type(ColorBlendType::Layer1)
                                .build(request, DrawPrimitive::TrianglesStrip),
                        );
                    }
                }
                let native = crate::layer::user::readback_test_texture(&mut context, &target);
                let different = actual
                    .chunks_exact(4)
                    .zip(native.chunks_exact(4))
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(
                    different, 0,
                    "{version} {label}: {different} pixel differences"
                );
            }
            eprintln!(
                "{version} {label}: {} glyphs, two backgrounds / 4147200 exact RGBA pixels",
                local.len()
            );
        }
    }
    std::fs::remove_dir_all(folder).unwrap();
}
