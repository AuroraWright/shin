use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    asset::{
        bustup::{BustupArgs, CharacterId, higurashi::HigurashiBustup},
        picture::{GpuPictureBlock, Picture},
    },
    layer::user::higurashi::fragment_tests::gpu as basic_gpu,
};
use binrw::BinWrite;
use glam::{Mat4, vec3};
use shin_core::{
    format::scenario::instruction_elements::UntypedNumberSpec, primitives::color::UnormColor,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosColVertex,
    },
};
use std::{
    io::Cursor,
    sync::atomic::{AtomicU32, Ordering},
};

fn number(code: &mut Vec<u8>, value: i32) {
    let mut bytes = Cursor::new(Vec::new());
    UntypedNumberSpec::Constant(value)
        .write_le(&mut bytes)
        .unwrap();
    code.extend(bytes.into_inner());
}
fn load(code: &mut Vec<u8>, id: i32, kind: i32, params: &[i32]) {
    code.push(0xc1);
    for v in [id, kind, 0] {
        number(code, v);
    }
    code.push(
        (1u16
            .checked_shl(params.len() as u32)
            .unwrap_or(0)
            .wrapping_sub(1)) as u8,
    );
    for &v in params {
        number(code, v);
    }
}
fn control(code: &mut Vec<u8>, id: i32, property: i32, value: i32) {
    code.push(0xc3);
    number(code, id);
    number(code, property);
    code.push(1);
    number(code, value);
}
fn scene_program(profile: usize) -> Vec<u8> {
    let mut code = Vec::new();
    load(&mut code, 7, 2, &[0]);
    load(&mut code, 9, 3, &[0]);
    load(&mut code, 11, 1, &[0xf123, -640, -720, 1280, 720]);
    control(&mut code, 7, 5, 50);
    control(&mut code, 9, 5, 30);
    control(&mut code, 11, 5, 80);
    if profile == 1 {
        control(&mut code, 9, 9, 700);
        control(&mut code, 9, 0, 75);
        control(&mut code, 7, 12, 1100);
        control(&mut code, -1, 0, 15);
        control(&mut code, -2, 1, -25);
    } else if profile == 2 {
        control(&mut code, 7, 21, 1);
        control(&mut code, 7, 23, 1000);
        control(&mut code, 7, 24, 900);
        control(&mut code, 7, 25, 800);
        code.extend([0xc5, 9]);
        control(&mut code, 9, 0, -90);
        code.extend([0x8e, 0, 0, 0, 0]);
    }
    code.extend([0xa0, 0, 2, b'A', 0, 0xa0, 1, 2, b'B', 0, 0xa1, 0, 0, 0]);
    code
}
enum Topology {
    Empty,
    Picture(Arc<Picture>),
    Bustup(Arc<HigurashiBustup>),
}
fn topology(server: &AssetServer, scenario: &HigurashiScenario, saved: &Hydrated) -> Vec<Topology> {
    saved
        .records
        .iter()
        .take_while(|r| r[0] >= 0)
        .map(|r| match r[0] {
            2 => {
                let info = &scenario.picture_info[r[6] as usize];
                let name =
                    crate::layer::user::higurashi::selected_asset_name(info.name.as_str(), 0);
                Topology::Picture(server.load_sync(format!("/picture/{name}.pic")).unwrap())
            }
            3 => {
                let info = &scenario.bustup_info[r[6] as usize];
                let name =
                    crate::layer::user::higurashi::selected_asset_name(info.name.as_str(), 0);
                Topology::Bustup(
                    shin_tasks::block_on(server.load_with_args(
                        format!("/bustup/{name}.bup"),
                        BustupArgs {
                            expression: info.emotion.as_str().to_owned(),
                            character_id: CharacterId::from_metadata(info.lipsync_character_id),
                            disable_animations: false,
                        },
                    ))
                    .unwrap(),
                )
            }
            _ => Topology::Empty,
        })
        .collect()
}
enum Packet {
    Block {
        stencil: u8,
        block: usize,
        matrix: Mat4,
        descriptor: [u32; 12],
    },
    Tile {
        stencil: u8,
        blend: u32,
        matrix: Mat4,
        vertices: [u32; 16],
    },
}
fn native<'a>(
    saved: &Hydrated,
    scene: &Scene,
    topology: &'a [Topology],
) -> (Vec<Packet>, Vec<&'a GpuPictureBlock>) {
    let mut input = format!("R {}", scene.layers.len());
    for (values, props) in [(&saved.root, &scene.root), (&saved.screen, &scene.screen)] {
        for v in values {
            write!(input, " {v}").unwrap();
        }
        for o in props.oscillators {
            write!(input, " {}", o.seed).unwrap();
        }
    }
    let mut blocks = Vec::new();
    let mut block = |owner: Option<&'a GpuPictureBlock>| -> u32 {
        if let Some(owner) = owner {
            blocks.push(owner);
            blocks.len() as u32
        } else {
            0
        }
    };
    for &(slot, _, ref owner) in &scene.layers {
        let record = &saved.records[usize::from(slot)];
        write!(input, " {} {}", record[0], record[1]).unwrap();
        for v in &record[6..14] {
            write!(input, " {v}").unwrap();
        }
        for v in &record[14..] {
            write!(input, " {v}").unwrap();
        }
        for o in group::Drawable::properties(owner).oscillators {
            write!(input, " {}", o.seed).unwrap();
        }
        match &topology[usize::from(slot)] {
            Topology::Empty => {
                input.push_str(" 0 0");
            }
            Topology::Picture(pic) => {
                let placements = pic.blocks.iter_placements().collect::<Vec<_>>();
                write!(
                    input,
                    " {} {} {}",
                    pic.origin_x,
                    pic.origin_y,
                    placements.len()
                )
                .unwrap();
                for (_, pos, owner) in placements {
                    write!(
                        input,
                        " {} {} {}",
                        block(Some(owner)),
                        pos.x.to_bits(),
                        pos.y.to_bits()
                    )
                    .unwrap();
                }
            }
            Topology::Bustup(bup) => {
                write!(
                    input,
                    " {} {} {}",
                    bup.origin_x,
                    bup.origin_y,
                    bup.body.blocks.len()
                )
                .unwrap();
                for owner in &bup.body.blocks {
                    write!(input, " {}", block(owner.as_deref())).unwrap();
                }
                for owner in [
                    bup.auxiliary.as_ref().and_then(|a| a.block.as_deref()),
                    bup.face.as_deref(),
                    bup.mouths[0].as_deref(),
                    bup.mouths[1].as_deref(),
                    bup.mouths[2].as_deref(),
                    bup.eye.as_deref(),
                ] {
                    write!(input, " {}", block(owner)).unwrap();
                }
            }
        }
    }
    input.push('\n');
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_RENDER").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap())
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
    let text = String::from_utf8(output.stdout).unwrap();
    let mut words = text.split_whitespace();
    assert_eq!(words.next(), Some("R"));
    let mut packets = Vec::new();
    loop {
        let kind = words.next().unwrap();
        if kind == "E" {
            assert_eq!(
                words.next().unwrap().parse::<usize>().unwrap(),
                packets.len()
            );
            break;
        }
        let _: u32 = words.next().unwrap().parse().unwrap();
        let stencil = words.next().unwrap().parse().unwrap();
        let _: u32 = words.next().unwrap().parse().unwrap();
        let id = words.next().unwrap().parse::<u32>().unwrap();
        let matrix = Mat4::from_cols_array(&std::array::from_fn(|_| {
            f32::from_bits(words.next().unwrap().parse().unwrap())
        }));
        if kind == "B" {
            packets.push(Packet::Block {
                stencil,
                block: id as usize - 1,
                matrix,
                descriptor: std::array::from_fn(|_| words.next().unwrap().parse().unwrap()),
            });
        } else {
            assert_eq!(kind, "T");
            packets.push(Packet::Tile {
                stencil,
                blend: id,
                matrix,
                vertices: std::array::from_fn(|_| words.next().unwrap().parse().unwrap()),
            });
        }
    }
    assert!(words.next().is_none());
    (packets, blocks)
}
/// Preserve the earlier native PIC/BUP/group packet order and matrices, then
/// execute the separate frozen Core oracle for material and indexed subranges.
/// Its synthetic full-canvas culling bounds are a checked transport boundary;
/// this comparison does not claim per-asset native block culling.
fn core_packets(
    profile: usize,
    packets: &[Packet],
    blocks: &[&GpuPictureBlock],
) -> Vec<Vec<basic_gpu::Draw>> {
    let mut input = String::new();
    let mut packet_indices = Vec::new();
    for (index, packet) in packets.iter().enumerate() {
        if let Packet::Block {
            block,
            descriptor: p,
            ..
        } = packet
        {
            assert!(p[0] <= 1 && p[1] <= 1 && p[6] <= 2 && p[7] <= 5);
            let source = blocks[*block];
            let opaque = source.opaque_rect_count * 6;
            let transparent = source.transparent_rect_count * 6;
            assert!(opaque <= u16::MAX as usize && transparent <= u16::MAX as usize);
            write!(
                input,
                "C {} {} {} {} {} {opaque} {transparent}",
                p[0], p[1], p[6], p[7], source.native_format
            )
            .unwrap();
            for word in p[2..6].iter().chain(&p[8..12]) {
                write!(input, " {word}").unwrap();
            }
            input.push('\n');
            packet_indices.push(index);
        }
    }
    let mut core = vec![vec![]; packets.len()];
    if packet_indices.is_empty() {
        return core;
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BASIC_FRAGMENTS").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let captured_input = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_BASIC_CORE_CAPTURE") {
        let directory = std::path::Path::new(&directory).join(format!("profile-{profile:02}"));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("input.txt"), captured_input).unwrap();
        std::fs::write(directory.join("output.txt"), &output.stdout).unwrap();
        std::fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
    }
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), packet_indices.len());
    for (row, (line, &packet_index)) in text.lines().zip(&packet_indices).enumerate() {
        let Packet::Block {
            stencil,
            matrix,
            descriptor,
            ..
        } = &packets[packet_index]
        else {
            unreachable!()
        };
        let mut tokens = line.split_whitespace();
        let read = |tokens: &mut std::str::SplitWhitespace<'_>| -> u32 {
            tokens.next().unwrap().parse().unwrap()
        };
        let read_words = |tokens: &mut std::str::SplitWhitespace<'_>, count: usize| {
            (0..count).map(|_| read(tokens)).collect::<Vec<_>>()
        };
        assert_eq!(tokens.next(), Some("G"));
        assert_eq!(read(&mut tokens), row as u32);
        let mut submissions = 0;
        let mut bindings = 0;
        loop {
            match tokens.next().unwrap() {
                "M" => assert_eq!(read_words(&mut tokens, 2), [5, 1]),
                "W" => assert_eq!(read_words(&mut tokens, 3), [7, 7, 7]),
                "P" => {
                    let p = read_words(&mut tokens, 30);
                    assert_eq!(&p[..2], &[0, descriptor[0]]);
                    assert_eq!(&p[2..14], descriptor);
                    submissions += 1;
                }
                "V" => assert!(read(&mut tokens) <= 1),
                "B" => {
                    assert_eq!(read_words(&mut tokens, 3), [0, 501, 101]);
                    bindings += 1;
                }
                "D" => {
                    let d = read_words(&mut tokens, 86);
                    assert_eq!(&d[..2], &[0, descriptor[0]]);
                    assert_eq!((d[4], d[7], d[8], d[9], d[10]), (0, 501, 101, 601, 701));
                    assert_eq!(&d[11..13], &[7, 0]);
                    assert_eq!(&d[18..21], &[64, 16, 16]);
                    assert_eq!(&d[38..42], &descriptor[2..6]);
                    assert_eq!(&d[54..58], &descriptor[8..12]);
                    core[packet_index].push(basic_gpu::Draw {
                        program: d[2],
                        blend: d[3],
                        index_start: d[6],
                        index_count: d[5],
                        // The first probe's native asset/group matrix supplies
                        // geometry. GLSL performs the single clip-Z bridge.
                        matrix: matrix.to_cols_array().map(f32::to_bits),
                        color: d[38..42].try_into().unwrap(),
                        param: d[54..58].try_into().unwrap(),
                        // Core inherits its caller's stencil; the original
                        // group/PIC/BUP packet supplies that reference.
                        stencil: u32::from(*stencil),
                    });
                }
                "E" => {
                    let e = read_words(&mut tokens, 5);
                    assert_eq!(
                        e,
                        [submissions, core[packet_index].len() as u32, bindings, 0, 0]
                    );
                    assert_eq!(submissions, 1);
                    assert!(tokens.next().is_none());
                    break;
                }
                event => panic!("Unreviewed original preview Core event{event}"),
            }
        }
    }
    core
}

fn replay(
    context: &mut PreRenderContext,
    target: TextureTarget<'_>,
    packets: &[Packet],
    blocks: &[&GpuPictureBlock],
    core: &[Vec<basic_gpu::Draw>],
) {
    assert_eq!(packets.len(), core.len());
    for (packet, core) in packets.iter().zip(core) {
        match packet {
            Packet::Block { block, .. } => {
                basic_gpu::replay(context, target.view, blocks[*block], core);
            }
            Packet::Tile {
                stencil,
                blend,
                matrix,
                vertices: v,
            } => {
                assert!(core.is_empty());
                let vertices: [PosColVertex; 4] = std::array::from_fn(|i| {
                    let c = v[4 * i + 3].to_le_bytes();
                    PosColVertex {
                        position: vec3(
                            f32::from_bits(v[4 * i]),
                            f32::from_bits(v[4 * i + 1]),
                            f32::from_bits(v[4 * i + 2]),
                        ),
                        color: UnormColor::from_rgba(c[2], c[1], c[0], c[3]),
                    }
                });
                // Keep attachment contents and original packet order across
                // raw block replay and the retained native Tile transport.
                let mut pass = context.begin_pass_preserving_depth_stencil(
                    target,
                    context.depth_stencil,
                    "Native saved preview Tile packet",
                );
                pass.run(
                    RenderRequestBuilder::new()
                        .depth_stencil_shorthand(*stencil, false, false)
                        .color_blend_type(match blend {
                            1 => ColorBlendType::Opaque,
                            2 => ColorBlendType::Layer1,
                            3 => ColorBlendType::Layer2,
                            4 => ColorBlendType::HigurashiLayer3,
                            _ => panic!("native Tile blend"),
                        })
                        .build(
                            RenderProgramWithArguments::Fill {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                transform: HigurashiTransform::convert_clip_depth(*matrix),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
        }
    }
}
fn expected_capture(
    context: &mut PreRenderContext,
    canvas: (u32, u32),
    packets: &[Packet],
    blocks: &[&GpuPictureBlock],
    core: &[Vec<basic_gpu::Draw>],
) -> RenderTexture {
    let (width, height) = resources::thumbnail_size(canvas);
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        width, height,
    )));
    let depth = context.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
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
    let depth_view = depth.create_view(&Default::default());
    let mut ctx = PreRenderContext {
        device: context.device,
        queue: context.queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: context.sampler_store,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: context.pipeline_storage,
        dynamic_buffer: context.dynamic_buffer,
        encoder: context.encoder,
    };
    let mut texture = ctx.new_render_texture("Native saved preview packet replay".into());
    {
        let mut pass = ctx.begin_pass(
            texture.as_texture_target(),
            Some(ctx.depth_stencil),
            "Native saved preview packets",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
    }
    replay(&mut ctx, texture.as_texture_target(), packets, blocks, core);
    texture
}
fn present(context: &mut PreRenderContext, view: &wgpu::TextureView, texture: &RenderTexture) {
    let mut pass = context.begin_pass(
        TextureTarget {
            kind: TextureTargetKind::Screen,
            view,
        },
        Some(context.depth_stencil),
        "Saved preview publication",
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

fn assert_pixels(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len(), "RGBA byte length:{label}");
    let different = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    if different != 0 {
        if let Ok(directory) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_BASIC_CORE_CAPTURE") {
            let directory = std::path::Path::new(&directory).join("failed-frames");
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("{label}-actual.rgba")), actual).unwrap();
            std::fs::write(directory.join(format!("{label}-original.rgba")), expected).unwrap();
        }
    }
    assert!(
        different == 0,
        "{label}: {different} RGBA channels differ,maximum{maximum}"
    );
}

#[test]
#[ignore = "actual saved VM + original native group/PIC/BUP/Tile packets; real generated228x128/342x192 thumbnails and720p/1080p publication; GPU offscreen only"]
fn higurashi_saved_preview_generated_pixels_match_native_720p_and_1080p() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let mut frames = 0;
    let mut packet_count = 0;
    let mut core_packet_count = 0;
    for profile in 0..3 {
        let scenario = scenario(&scene_program(profile));
        let data = data(0, 1, 999, 42);
        let saved = Hydrated::replay(&scenario, &data).unwrap();
        let seed = Arc::new(AtomicU32::new(20));
        let seeds: SeedSource = {
            let seed = seed.clone();
            Arc::new(move || seed.fetch_add(1, Ordering::SeqCst))
        };
        let scene = shin_tasks::block_on(Scene::load(
            &server,
            &scenario,
            &saved,
            [[1, 2, 3, 4], [5, 6, 7, 8]],
            &seeds,
            0,
        ))
        .unwrap();
        assert_eq!(
            seed.load(Ordering::SeqCst),
            20 + 4 * scene.layers.len() as u32
        );
        let topology = topology(&server, &scenario, &saved);
        let (packets, blocks) = native(&saved, &scene, &topology);
        assert!(!packets.is_empty());
        packet_count += packets.len();
        let core = core_packets(profile, &packets, &blocks);
        core_packet_count += core.iter().map(Vec::len).sum::<usize>();
        for canvas in [(1280, 720), (1920, 1080)] {
            let thumb = resources::thumbnail_size(canvas);
            let actual = reference_render_to_texture(&device, &queue, thumb, |ctx, view| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view,
                    },
                    Some(ctx.depth_stencil),
                    "Saved scene",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                scene.render(&mut pass).unwrap();
            });
            let expected = reference_render_to_texture(&device, &queue, thumb, |ctx, view| {
                let target = TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view,
                };
                {
                    let mut pass = ctx.begin_pass(
                        target,
                        Some(ctx.depth_stencil),
                        "Native saved scene initial clear",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                }
                replay(ctx, target, &packets, &blocks, &core);
            });
            assert_pixels(
                &actual,
                &expected,
                &format!("profile-{profile:02}-thumbnail-{}x{}", thumb.0, thumb.1),
            );
            assert!(
                actual.chunks_exact(4).any(|c| c[..3] != [0, 0, 0]),
                "Generated preview must contain scene pixels"
            );
            frames += 1;
            let actual = reference_render_to_texture(&device, &queue, canvas, |ctx, view| {
                let texture = scene.capture(ctx, canvas).unwrap();
                assert_eq!(texture.dimensions(), thumb);
                present(ctx, view, &texture);
            });
            let expected = reference_render_to_texture(&device, &queue, canvas, |ctx, view| {
                let texture = expected_capture(ctx, canvas, &packets, &blocks, &core);
                assert_eq!(texture.dimensions(), thumb);
                present(ctx, view, &texture);
            });
            assert_pixels(
                &actual,
                &expected,
                &format!("profile-{profile:02}-publication-{}x{}", canvas.0, canvas.1),
            );
            frames += 1;
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "Saved previews:{frames} byte-identical real generated thumbnail/publication frames at720p/1080p;{packet_count} original native root/screen/group/PIC/BUP/Tile packets;{core_packet_count} independent original Core indexed packets with primary GLSL/source-format ranges; physical targets228x128/342x192; native asset-block culling outside scope; no nodata/audio/windows/user saves"
    );
}

#[test]
#[ignore = "actual native d3710 CPU worker; bounded factories and GPU service; no audio/windows/saves"]
fn higurashi_saved_preview_native_capture_clone_and_retirement_contracts() {
    let input = (0..33)
        .flat_map(|count| (0..2).map(move |cancel| format!("C {count} {cancel}\n")))
        .collect::<String>();
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_CAPTURE").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap())
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
    assert_eq!(output.lines().count(), 66);
    for (case, line) in output.lines().enumerate() {
        let expected = if case % 2 == 0 {
            format!("C 1 32 {} {} {}", case / 2, case / 2, case)
        } else {
            "C 0 32 0 0 0".into()
        };
        assert_eq!(line, expected, "native capture case{case}");
    }
    eprintln!(
        "Saved preview:66 actual original d3710 clone/property/ID/BUPoverride/suspend/publication/cancellation owners; bounded factory/GPU services; balanced refretirement"
    );
}

#[test]
#[ignore = "retained saved VM/actual asset workers/capture publication/cache eviction/re-entry; offscreen GPU, no audio/windows/user saves"]
fn higurashi_saved_preview_retained_worker_publication_invalidation_and_reentry() {
    use crate::adv::higurashi::{browser_controls::Scrollbar, save_menu::model::Mode};
    use shin_core::format::save::higurashi::SaveData;
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let scenario = scenario(&scene_program(1));
    let mut slots = Slots::loaded(&SaveData::default());
    slots.manual[0].valid = true;
    slots.manual[0].data = data(0, 1, 999, 42);
    let counter = Arc::new(AtomicU32::new(1));
    let seeds: SeedSource = {
        let counter = counter.clone();
        Arc::new(move || counter.fetch_add(1, Ordering::SeqCst))
    };
    let mut service = PreviewService::new(server, scenario, (1280, 720), false, 0, seeds);
    assert_eq!(counter.load(Ordering::SeqCst), 17);
    let mut state = ViewState {
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
    };
    service.poll_rows(&slots, &state).unwrap();
    assert_eq!(service.rows[0].labels, Labels::Loading);
    assert!(service.thumbnails[0].is_none());
    let wait = |service: &mut PreviewService, state: &ViewState| {
        let started = std::time::Instant::now();
        loop {
            service.poll_rows(&slots, state).unwrap();
            if service.capture.as_ref().is_some_and(|c| c.ready.is_some()) {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(30),
                "Saved preview worker timeout"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    };
    wait(&mut service, &state);
    assert!(
        matches!(&service.rows[0].labels,Labels::Ready {title,subtitle} if title.as_ref()=="A"&&subtitle.as_ref()=="B")
    );
    assert!(
        service.thumbnails[0].is_none(),
        "CPU readiness must not fabricate GPU publication"
    );
    let cached = service.cached[0].clone().unwrap();
    let source = slots.manual[0].data.clone();
    let first = reference_render_to_texture(&device, &queue, (1280, 720), |ctx, view| {
        service.prepare(ctx, &state).unwrap();
        assert_eq!(
            service.thumbnails[0].as_ref().unwrap().dimensions(),
            (228, 128)
        );
        present(ctx, view, service.thumbnails[0].as_ref().unwrap());
    });
    assert!(first.chunks_exact(4).any(|c| c[..3] != [0, 0, 0]));
    assert_eq!(slots.manual[0].data, source);
    state.scroll = 5000.;
    service.poll_rows(&slots, &state).unwrap();
    assert!(service.thumbnails[0].is_none());
    assert!(Arc::ptr_eq(&cached, service.cached[0].as_ref().unwrap()));
    assert_eq!(service.rows[0], Row::default());
    state.scroll = 0.;
    wait(&mut service, &state);
    let second = reference_render_to_texture(&device, &queue, (1280, 720), |ctx, view| {
        service.prepare(ctx, &state).unwrap();
        present(ctx, view, service.thumbnails[0].as_ref().unwrap());
    });
    assert_pixels(&first, &second, "retained-preview-eviction-reentry");
    assert!(Arc::ptr_eq(&cached, service.cached[0].as_ref().unwrap()));
    // Invalidation's two native gates are independent. No slot version is used.
    service.thumbnails[0] = None;
    service.poll_rows(&slots, &state).unwrap();
    assert_eq!(service.capture.as_ref().unwrap().index, Some(0));
    service.invalidate(0);
    assert_eq!(service.capture.as_ref().unwrap().index, None);
    assert!(service.cached[0].is_none());
    service.poll_rows(&slots, &state).unwrap();
    if let Some(h) = &mut service.hydration {
        assert_eq!(h.index, 0);
        service.invalidate(0);
        assert!(service.hydration.as_ref().unwrap().cancelled);
    }
    drop(service);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "Saved preview retained owner:Loading→hydrated-after-rowloop→Ready→CPUcapture-ready→GPUpublication; exact generated pixels after thumbnail eviction/re-entry; VM retained, source unchanged, matching native index/cancel gates and worker retirement"
    );
}
