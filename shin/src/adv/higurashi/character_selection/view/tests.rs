use super::*;
use crate::{
    adv::higurashi::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame},
    layer::user::higurashi::{HigurashiLayerKind, HigurashiUserLayer},
};
use shin_core::primitives::color::UnormColor;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

struct Draw {
    program: u32,
    texture: u8,
    blend: u32,
    stencil: [u32; 5],
    matrix: [u32; 16],
    flash: [u32; 16],
    vertices: [u32; 24],
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn native(input: String) -> Vec<Vec<Draw>> {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHARACTER_RENDER").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copy.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_CHARACTER_RENDER_CAPTURE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), input).unwrap();
        std::fs::write(dir.join("output.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let mut w = line.split_whitespace();
            assert_eq!(w.next(), Some("R"));
            let mut draws = Vec::new();
            loop {
                match w.next().unwrap() {
                    "P" => draws.push(Draw {
                        program: word(&mut w),
                        texture: word(&mut w) as u8,
                        blend: word(&mut w),
                        stencil: words(&mut w),
                        matrix: words(&mut w),
                        flash: words(&mut w),
                        vertices: words(&mut w),
                    }),
                    "E" => break,
                    x => panic!("draw{x}"),
                }
            }
            assert_eq!(word(&mut w) as usize, draws.len());
            assert!(w.next().is_none());
            draws
        })
        .collect()
}
fn rgba(v: u32) -> u32 {
    (v & 0xff00ff00) | ((v & 255) << 16) | ((v >> 16) & 255)
}
#[derive(Clone)]
struct Values {
    selected: usize,
    route: bool,
    kind: PassKind,
    clocks: [f32; 8],
    rows: Vec<(f32, f32)>,
    properties: Vec<(usize, f32)>,
}
fn apply(layer: &mut HigurashiUserLayer, v: &Values) {
    for &(p, value) in &v.properties {
        layer.properties.control(p, value, Ticks::ZERO, 0).unwrap();
    }
    let view = layer.character_view_mut().unwrap();
    view.route_navigation = v.route;
    let mut model = view.model.lock();
    model.selected = v.selected as i32;
    for (c, &value) in model.clocks.iter_mut().zip(&v.clocks) {
        c.fast_forward_to(value);
    }
    for (row, &(alpha, flash)) in model.rows.iter_mut().zip(&v.rows) {
        row.alpha.fast_forward_to(alpha);
        row.flash.fast_forward_to(flash);
    }
}
fn check(draws: &[Draw], view: &View, properties: &HigurashiProperties, kind: PassKind) -> usize {
    if kind == PassKind::Opaque {
        assert!(draws.is_empty());
        return 0;
    }
    let packets = view.packets();
    assert_eq!(draws.len(), packets.len());
    let matrix = View::matrix(properties, &HigurashiTransform::default())
        .to_cols_array()
        .map(f32::to_bits);
    for (d, p) in draws.iter().zip(packets) {
        assert_eq!(
            (d.program, d.texture, d.blend),
            (if p.flash > 0.0 { 5 } else { 2 }, p.texture, 2)
        );
        assert_eq!(d.stencil, [6, 0, 0, 2, 0x3ffff]);
        assert_eq!(d.matrix, matrix, "character matrix");
        let mut vertices = d.vertices;
        for i in (3..24).step_by(6) {
            vertices[i] = rgba(vertices[i]);
        }
        assert_eq!(
            vertices,
            bytemuck::cast_slice::<_, u32>(&p.vertices()),
            "character texture{}",
            p.texture
        );
        if d.program == 5 {
            assert_eq!(
                d.flash,
                vec4(1.0, 1.0, 1.0, p.flash)
                    .to_array()
                    .into_iter()
                    .chain([0.0; 12])
                    .map(f32::to_bits)
                    .collect::<Vec<_>>()
                    .as_slice()
            );
        }
    }
    draws.len()
}
fn replay(pass: &mut RenderPass, draws: &[Draw], view: &View) {
    for d in draws {
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.matrix.map(f32::from_bits),
        ));
        let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| {
            let v = &d.vertices[i * 6..];
            PosColTexVertex {
                position: vec3(
                    f32::from_bits(v[0]),
                    f32::from_bits(v[1]),
                    f32::from_bits(v[2]),
                ),
                texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                color: UnormColor(rgba(v[3])),
            }
        });
        let vertices = VertexSource::VertexData {
            vertices: &vertices,
        };
        let texture = view.texture(d.texture).as_source();
        let program = if d.program == 5 {
            RenderProgramWithArguments::Button {
                vertices,
                texture,
                transform,
                flash1: Vec4::from_array(std::array::from_fn(|i| f32::from_bits(d.flash[i]))),
                flash2: Vec4::ZERO,
            }
        } else {
            RenderProgramWithArguments::Sprite {
                vertices,
                sprite: texture,
                transform,
            }
        };
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .depth_stencil_shorthand((d.stencil[4] >> 16) as u8, true, false)
                .build(program, DrawPrimitive::TrianglesStrip),
        );
    }
}
#[test]
#[ignore = "actual444b4 atlas/shadow/name/flash/caption/route packets and real scene user layer at720p/1080p on offscreen Metal,no audio or saves"]
fn higurashi_character_selection_renderer_matches_native_packets_and_pixels() {
    let (device, queue, server) = gpu_assets();
    let message: Arc<HigurashiMessageTextures> = server.load_sync("/msgtex.txa").unwrap();
    let mut cases = Vec::new();
    let mut input = String::new();
    for mask in [1u32, 2, 4, 8, 5, 7, 13, 15] {
        let view = shin_tasks::block_on(View::load(&server, message.clone(), mask)).unwrap();
        let count = view.model.lock().rows.len();
        let mut layer = HigurashiUserLayer::character_selection(view, [1, 2, 3, 4]);
        layer.properties.control(22, 1.0, Ticks::ZERO, 0).unwrap();
        layer.properties.control(5, 1.0, Ticks::ZERO, 0).unwrap();
        assert_eq!(layer.layer_type(), HigurashiLayerKind::CharacterSelection);
        writeln!(input, "N {mask:x}").unwrap();
        let mut values = Vec::new();
        for profile in 0..6 {
            let v = Values {
                selected: profile % count,
                route: profile != 3,
                kind: if profile == 5 {
                    PassKind::Opaque
                } else {
                    PassKind::Transparent
                },
                clocks: if profile == 0 {
                    [0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0]
                } else if profile == 1 {
                    [17.25, 0.375, 91.125, 0.4, 0.725, 0.8, 0.35, 0.65]
                } else if profile == 2 {
                    [0.0, 0.0, 300.0, 0.0, 0.0, 1.0, 1.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0]
                },
                rows: (0..count)
                    .map(|i| {
                        if profile == 1 {
                            (0.35 + i as f32 * 0.1, if i == 0 { 0.7 } else { 0.0 })
                        } else {
                            (1.0, 0.0)
                        }
                    })
                    .collect(),
                properties: if profile == 4 {
                    vec![
                        (0, 45.5),
                        (1, -17.25),
                        (12, 975.0),
                        (13, 1025.0),
                        (16, 21.0),
                    ]
                } else {
                    Vec::new()
                },
            };
            writeln!(
                input,
                "R {} {} 3 {} {}",
                v.selected,
                u8::from(v.route),
                u8::from(v.kind == PassKind::Opaque),
                v.properties.len()
            )
            .unwrap();
            for value in v.clocks {
                write!(input, "{:x} ", value.to_bits()).unwrap();
            }
            input.push('\n');
            for &(p, value) in &v.properties {
                writeln!(input, "{p} {:x}", value.to_bits()).unwrap();
            }
            for &(alpha, flash) in &v.rows {
                writeln!(input, "{:x} {:x}", alpha.to_bits(), flash.to_bits()).unwrap();
            }
            values.push(v);
        }
        input.push_str("D\n");
        cases.push((layer, values));
    }
    let captured = native(input);
    let mut captured = captured.iter();
    let mut number = 0;
    let mut packets = 0;
    let mut compared = 0;
    let mut hashes = String::new();
    let mut textures = BTreeMap::new();
    for (layer, values) in &mut cases {
        for v in values {
            apply(layer, v);
            let view = layer.character_view().unwrap();
            let draws = captured.next().unwrap();
            packets += check(draws, view, &layer.properties, v.kind);
            for d in draws {
                *textures.entry(d.texture).or_insert(0usize) += 1;
            }
            for size in [(1280, 720), (1920, 1080)] {
                let clear = UnormColor(0xff503020);
                let expected = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| replay(pass, draws, view),
                );
                let actual = render_prepared_frame(
                    &device,
                    &queue,
                    size,
                    clear,
                    |_| {},
                    |pass| {
                        layer
                            .render(pass, &HigurashiTransform::default(), 3, v.kind)
                            .unwrap()
                    },
                );
                assert_eq!(
                    actual.iter().zip(&expected).position(|(a, b)| a != b),
                    None,
                    "character frame{number} {size:?}"
                );
                let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(0x100000001b3)
                });
                writeln!(hashes, "{number} {} {} {hash:016x}", size.0, size.1).unwrap();
                compared += size.0 as usize * size.1 as usize;
            }
            number += 1;
        }
    }
    assert!(captured.next().is_none());
    assert_eq!(textures.len(), 12);
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_CHARACTER_PIXEL_HASHES") {
        std::fs::write(path, hashes).unwrap();
    }
    // Native434fc clones independent clocks while retaining atlas resources.
    let layer = &cases.last().unwrap().0;
    let copied = layer.clone();
    let original = layer.character_view().unwrap().model.clone();
    let copy = copied.character_view().unwrap().model.clone();
    assert!(!Arc::ptr_eq(&original, &copy));
    let before = copy.lock().clocks[1].value();
    original.lock().clocks[1].fast_forward_to(0.123);
    assert_eq!(copy.lock().clocks[1].value(), before);
    eprintln!(
        "Character renderer:8 owners,{number} logical frames,{} byte-identical720p/1080p RGBA frames,{packets} exact sprite/button packets,{compared} compared pixels;all12 atlas names,mask/portrait/shadow/caption/name/flash/route geometry,property transforms/opaque gating and scene-layer independent clone pass",
        number * 2
    );
}
