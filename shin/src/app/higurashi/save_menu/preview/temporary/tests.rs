use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use shin_core::time::Ticks;
use shin_render::{
    gpu_texture::GpuTexture,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Clone, Debug)]
struct Case {
    factory: bool,
    kind: i32,
    argument: i32,
    route: bool,
    pattern: u32,
    ids: Vec<usize>,
    pixels: bool,
}
impl Case {
    fn params(&self) -> [i32; 8] {
        let mut p = [0; 8];
        p[0] = self.argument;
        if self.kind == 12 {
            p[1] = self.ids.len() as i32;
            for (p, &id) in p[2..].iter_mut().zip(&self.ids) {
                *p = id as i32;
            }
        }
        p
    }
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for factory in [false, true] {
        for (index, argument) in [1, 2, 4, 8, 5, 7, 13, 15].into_iter().enumerate() {
            for route in [false, true] {
                cases.push(Case {
                    factory,
                    kind: 11,
                    argument,
                    route,
                    pattern: 0,
                    ids: vec![],
                    pixels: factory && route == (index % 2 == 0),
                });
            }
        }
        for argument in [0, 1] {
            for pattern in 0..5 {
                for count in 0..7 {
                    cases.push(Case {
                        factory,
                        kind: 12,
                        argument,
                        route: false,
                        pattern,
                        ids: [0, 1, 17, 33, 244, 314][..count].to_vec(),
                        pixels: factory
                            && argument == 1
                            && [(0, 0), (0, 1), (3, 3), (4, 6)].contains(&(pattern, count)),
                    });
                }
            }
        }
    }
    // The later occurrence observes the first occurrence's write. Pattern4
    // also preserves previously read bits for IDs17 and acquired state for17.
    for argument in [0, 1] {
        for pattern in [0, 4] {
            cases.push(Case {
                factory: true,
                kind: 12,
                argument,
                route: false,
                pattern,
                ids: vec![0, 0, 17, 0, 17, 244],
                pixels: argument == 0,
            });
        }
    }
    cases
}

#[derive(Clone, Debug)]
struct Bits {
    words: Vec<u32>,
    writes: Vec<usize>,
}
impl Bits {
    fn pattern(pattern: u32) -> Self {
        let mut words = vec![0; 20];
        for id in 0..315 {
            let status = if pattern == 4 {
                id % 4
            } else {
                pattern as usize
            };
            for lane in 0..2 {
                if status & (1 << lane) != 0 {
                    let bit = id * 2 + lane;
                    words[bit / 32] |= 1 << (bit % 32);
                }
            }
        }
        Self {
            words,
            writes: vec![],
        }
    }
}
impl AcquiredBits for Bits {
    fn acquired(&self, id: usize) -> bool {
        let bit = 2 * id;
        self.words
            .get(bit / 32)
            .is_some_and(|word| word & (1 << (bit % 32)) != 0)
    }
    fn acquire(&mut self, id: usize) -> Result<()> {
        let bit = 2 * id;
        let index = bit / 32;
        if index >= self.words.len() {
            self.words.resize((index / 256 + 1) * 256, 0);
        }
        self.words[index] |= 1 << (bit % 32);
        self.writes.push(id);
        Ok(())
    }
}

#[derive(Debug)]
struct Draw {
    program: u32,
    blend: u32,
    texture: u32,
    stencil: [u32; 2],
    uniforms: [[u32; 16]; 3],
    vertices: Vec<u32>,
}
#[derive(Debug)]
struct Native {
    kind: i32,
    seeds: u32,
    phase: u32,
    rows: usize,
    bits: [u32; 20],
    clocks: Vec<u32>,
    alpha: Vec<u32>,
    draws: Vec<Draw>,
    textures: BTreeMap<u32, String>,
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn native(cases: &[Case]) -> Vec<Native> {
    let mut input = String::new();
    for c in cases {
        write!(
            input,
            "{} {} {} {} {} {}",
            if c.factory { 'F' } else { 'N' },
            c.kind,
            c.argument,
            c.ids.len(),
            u8::from(c.route),
            c.pattern
        )
        .unwrap();
        for id in &c.ids {
            write!(input, " {id}").unwrap();
        }
        input.push('\n');
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_TEMPORARY").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
                std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap(),
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_TEMPORARY_CAPTURE") {
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
            assert_eq!(w.next(), Some("N"));
            let kind = word(&mut w) as i32;
            let seeds = word(&mut w);
            let phase = word(&mut w);
            let rows = word(&mut w) as usize;
            let bits = words(&mut w);
            assert_eq!(w.next(), Some("C"));
            let clocks = (0..if kind == 11 { 8 } else { 2 })
                .map(|_| word(&mut w))
                .collect();
            let alpha = (0..rows).map(|_| word(&mut w)).collect();
            assert_eq!(w.next(), Some("R"));
            let mut draws = Vec::new();
            loop {
                match w.next().unwrap() {
                    "P" => {
                        let program = word(&mut w);
                        draws.push(Draw {
                            program,
                            blend: word(&mut w),
                            texture: word(&mut w),
                            stencil: words(&mut w),
                            uniforms: std::array::from_fn(|_| words(&mut w)),
                            vertices: (0..if program == 1 { 16 } else { 24 })
                                .map(|_| word(&mut w))
                                .collect(),
                        });
                    }
                    "E" => break,
                    p => panic!("Native temporary packet{p}"),
                }
            }
            assert_eq!(word(&mut w) as usize, draws.len());
            assert_eq!(w.next(), Some("K"));
            let textures = (0..word(&mut w))
                .map(|_| (word(&mut w), w.next().unwrap().to_owned()))
                .collect();
            assert!(w.next().is_none());
            Native {
                kind,
                seeds,
                phase,
                rows,
                bits,
                clocks,
                alpha,
                draws,
                textures,
            }
        })
        .collect()
}
fn rgba(v: u32) -> u32 {
    (v & 0xff00ff00) | ((v & 255) << 16) | ((v >> 16) & 255)
}
fn texture_name(id: u8) -> String {
    match id {
        0..=3 => format!("shadow{id}"),
        4..=7 => format!("char{}", id - 4),
        8 => "name".into(),
        9 => "caption".into(),
        10 => "navi_arrow".into(),
        11 => "navi_icon".into(),
        _ => panic!("Character atlas ID{id}"),
    }
}
fn properties() -> HigurashiProperties {
    let mut properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
    properties.control(5, 1., Ticks::ZERO, 0).unwrap();
    properties.control(22, 1., Ticks::ZERO, 0).unwrap();
    properties
}
fn check(case: usize, view: &View, native: &Native, properties: &HigurashiProperties) -> usize {
    let packets: Vec<(u32, u32, Option<String>, Vec<u32>)> = match view {
        View::Character(view) => {
            let m = view.model.lock();
            assert_eq!(m.phase, native.phase);
            assert_eq!(m.rows.len(), native.rows);
            assert_eq!(m.selected, 0);
            assert_eq!(
                m.clocks
                    .iter()
                    .map(|c| c.value().to_bits())
                    .collect::<Vec<_>>(),
                native.clocks,
                "case{case} character virtual40 clocks"
            );
            assert_eq!(
                m.rows
                    .iter()
                    .map(|r| r.alpha.value().to_bits())
                    .collect::<Vec<_>>(),
                native.alpha
            );
            drop(m);
            view.reference_packets()
                .into_iter()
                .map(|(texture, vertices, flash)| {
                    assert_eq!(flash, 0.);
                    (
                        2,
                        2,
                        Some(texture_name(texture)),
                        bytemuck::cast_slice(&vertices).to_vec(),
                    )
                })
                .collect()
        }
        View::Tips(view) => {
            assert_eq!(native.phase, 0);
            assert_eq!(native.clocks, [0f32.to_bits(), 160f32.to_bits()]);
            assert_eq!(view.rows.len(), native.rows);
            assert_eq!(native.alpha, vec![1f32.to_bits(); native.rows]);
            [PassKind::Opaque, PassKind::Transparent]
                .into_iter()
                .flat_map(|kind| view.reference_packets(kind))
                .map(|packet| match &packet {
                    Packet::Fill { opaque, .. } => (
                        1,
                        if *opaque { 1 } else { 2 },
                        None,
                        bytemuck::cast_slice(&packet.fill_vertices()).to_vec(),
                    ),
                    Packet::Sprite { texture, .. } => (
                        2,
                        2,
                        Some(texture.clone()),
                        bytemuck::cast_slice(&packet.sprite_vertices()).to_vec(),
                    ),
                })
                .collect()
        }
    };
    assert_eq!(packets.len(), native.draws.len(), "case{case} packet count");
    let matrix = TipView::matrix(properties, &HigurashiTransform::default())
        .to_cols_array()
        .map(f32::to_bits);
    for (index, (d, (program, blend, texture, vertices))) in
        native.draws.iter().zip(&packets).enumerate()
    {
        assert_eq!(d.program, *program, "case{case} draw{index} program");
        assert_eq!(d.blend, *blend, "case{case} draw{index} blend");
        assert_eq!(d.stencil, [7, 1], "case{case} draw{index} stencil");
        assert_eq!(d.uniforms[0], matrix, "case{case} draw{index} transform");
        assert_eq!(d.uniforms[1..], [[0; 16]; 2]);
        assert_eq!(native.textures.get(&d.texture), texture.as_ref());
        let mut original = d.vertices.clone();
        for i in (3..original.len()).step_by(if d.program == 1 { 4 } else { 6 }) {
            original[i] = rgba(original[i]);
        }
        assert_eq!(
            &original, vertices,
            "case{case} draw{index} exact vertices/UV"
        );
    }
    packets.len()
}

struct Textures {
    character: Arc<CharacterTextures>,
    tips: Arc<CharacterTextures>,
    message: Arc<HigurashiMessageTextures>,
}
impl Textures {
    fn get(&self, kind: i32, name: &str) -> &GpuTexture {
        match (kind, name) {
            (11, "navi_arrow") => &self.message.navigation_arrows,
            (11, "navi_icon") => &self.message.navigation_icons,
            (11, name) => self.character.get(name),
            (12, name) => self.tips.get(name),
            _ => panic!("Native temporary texture kind{kind}"),
        }
    }
}
fn replay(pass: &mut RenderPass, native: &Native, textures: &Textures) {
    replay_at_stencil(pass, native, textures, None, None)
}
fn replay_at_stencil(
    pass: &mut RenderPass,
    native: &Native,
    textures: &Textures,
    stencil: Option<u8>,
    kind: Option<PassKind>,
) {
    for d in &native.draws {
        let native_kind = if d.program == 1 && d.blend == 1 {
            PassKind::Opaque
        } else {
            PassKind::Transparent
        };
        if kind.is_some_and(|kind| kind != native_kind) {
            continue;
        }
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.uniforms[0].map(f32::from_bits),
        ));
        let builder = RenderRequestBuilder::new()
            .color_blend_type(match d.blend {
                1 => ColorBlendType::Opaque,
                2 => ColorBlendType::Layer1,
                b => panic!("Native temporary blend{b}"),
            })
            .depth_stencil_shorthand(
                stencil.unwrap_or(d.stencil[0] as u8),
                d.stencil[1] != 0,
                false,
            );
        if d.program == 1 {
            let vertices: [PosColVertex; 4] = std::array::from_fn(|i| {
                let v = &d.vertices[4 * i..];
                PosColVertex {
                    position: vec3(
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ),
                    color: UnormColor(rgba(v[3])),
                }
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            assert_eq!(d.program, 2);
            let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| {
                let v = &d.vertices[6 * i..];
                PosColTexVertex {
                    position: vec3(
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ),
                    color: UnormColor(rgba(v[3])),
                    texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                }
            });
            pass.run(
                builder.build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: textures
                            .get(native.kind, &native.textures[&d.texture])
                            .as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
            );
        }
    }
}

/// Actual original temporary packets with only the caller stencil remapped.
/// For retained-scene tests, refreshed property22=1 bypasses parent matrices.
pub(in crate::app::higurashi::save_menu::preview) struct Reference {
    native: Native,
    textures: Textures,
}
impl Reference {
    pub(in crate::app::higurashi::save_menu::preview) fn load(
        server: &AssetServer,
        kind: i32,
        argument: i32,
        ids: &[usize],
        route: bool,
        pattern: u32,
    ) -> Self {
        let mut output = native(&[Case {
            factory: true,
            kind,
            argument,
            route,
            pattern,
            ids: ids.to_vec(),
            pixels: false,
        }]);
        Self {
            native: output.remove(0),
            textures: Textures {
                character: server.load_sync("/charsel.txa").unwrap(),
                tips: server.load_sync("/tipsget.txa").unwrap(),
                message: server.load_sync("/msgtex.txa").unwrap(),
            },
        }
    }
    pub(in crate::app::higurashi::save_menu::preview) fn render(
        &self,
        pass: &mut RenderPass,
        stencil: u8,
        kind: Option<PassKind>,
    ) {
        replay_at_stencil(pass, &self.native, &self.textures, Some(stencil), kind);
    }
    pub(in crate::app::higurashi::save_menu::preview) fn packet_count(&self) -> usize {
        self.native.draws.len()
    }
}

#[test]
#[ignore = "actual updated type11/type12 factories/clone/refresh/packets/global bits and real asset offscreen720p/1080p; no audio/windows/user saves"]
fn higurashi_saved_preview_temporaries_match_native_packets_bits_and_pixels() {
    let cases = cases();
    assert_eq!(cases.len(), 176);
    let original = native(&cases);
    assert_eq!(original.len(), cases.len());
    let (device, queue, server) = reference_gpu_assets();
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let textures = Textures {
        character: server.load_sync("/charsel.txa").unwrap(),
        tips: server.load_sync("/tipsget.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
    };
    let properties = properties();
    let mut packets = 0;
    let mut mutations = 0;
    let mut frames = 0;
    for (case, (c, n)) in cases.iter().zip(&original).enumerate() {
        assert_eq!((n.kind, n.seeds), (c.kind, 4));
        let mut bits = Bits::pattern(c.pattern);
        let before = bits.clone();
        let prepared = shin_tasks::block_on(Prepared::load(
            &server,
            &scenario,
            c.kind,
            &c.params(),
            c.route,
        ))
        .unwrap();
        assert_eq!(
            bits.words, before.words,
            "async factory changed live globals"
        );
        let view = prepared.clone_refresh(&mut bits).unwrap();
        assert_eq!(bits.words, n.bits, "case{case} acquired/read/padding bits");
        let mut expected = before.clone();
        for &id in &c.ids {
            if !expected.acquired(id) {
                expected.acquire(id).unwrap();
            }
        }
        assert_eq!(
            bits.writes, expected.writes,
            "case{case} per-ID mutation order"
        );
        mutations += bits.writes.len();
        packets += check(case, &view, n, &properties);
        if c.kind == 12 {
            let second = prepared.clone_refresh(&mut bits).unwrap();
            let View::Tips(second) = second else {
                unreachable!()
            };
            assert!(second.rows.iter().all(|r| r.previously_acquired));
            assert_eq!(
                bits.writes, expected.writes,
                "reclone must not reset live bits"
            );
        }
        if !c.pixels {
            continue;
        }
        for size in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: target,
                    },
                    Some(ctx.depth_stencil),
                    "Saved temporary actual refreshed drawable",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    view.render(
                        &mut pass,
                        &properties,
                        &HigurashiTransform::default(),
                        7,
                        kind,
                    )
                    .unwrap();
                }
            });
            let expected = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: target,
                    },
                    Some(ctx.depth_stencil),
                    "Saved temporary original refreshed packets",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                replay(&mut pass, n, &textures);
            });
            assert_eq!(
                actual, expected,
                "case{case} {size:?} exact original replay pixels"
            );
            assert!(actual.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
            frames += 1;
        }
    }
    assert_eq!(frames, 28);
    eprintln!(
        "Saved preview temporaries:{} actual factory/constructor/clone/refresh owners,{packets} exact packets,{mutations} ordered missing-bit writes,duplicate-ID/read/padding/reclone checks; {frames} byte-identical full720p/1080p real chars/msgtex/tipsget frames; checked atlas/property/GPU services,no audio/windows/saves",
        cases.len()
    );
}
