//! Independent original shader replay of leaf masks and real basic materials.
//! Pure offscreen GPU test seams; no app/audio/window/user-save initialization.
use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
    sync::Arc,
};

use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    asset::{
        bustup::{BustupArgs, CharacterId, higurashi::HigurashiBustup},
        picture::Picture,
    },
    layer::{
        higurashi_properties::INITIAL_VALUES,
        render_params::higurashi::native_rotation_z,
        user::{
            higurashi::{HigurashiUserLayer, block, selected_asset_name, tile::Tile},
            picture_layer::PictureBlockParams,
        },
    },
    render::PreRenderContext,
};
use glam::{Vec3, vec2, vec4};
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::FloatColor4};
use shin_render::{
    PassKind,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};

pub(crate) mod gpu;

#[derive(Clone)]
pub(crate) struct OriginalClip {
    pub(crate) draws: Vec<gpu::Draw>,
    pub(crate) material_depth: [u32; 2],
    pub(crate) material_stencil: [u32; 5],
}
fn word(tokens: &mut std::str::SplitWhitespace<'_>) -> u32 {
    tokens.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(tokens: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(tokens))
}
fn run(env: &str, input: String, font: bool, capture: &str) -> String {
    static CAPTURE_SEQUENCE: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut command = Command::new(std::env::var(env).unwrap());
    command.arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap());
    if font {
        command.arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap());
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copied = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_LEAF_CLIP_GPU_CAPTURE") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(
            directory.join(format!("{capture}-{sequence}-input.txt")),
            copied,
        )
        .unwrap();
        std::fs::write(
            directory.join(format!("{capture}-{sequence}-output.txt")),
            &output.stdout,
        )
        .unwrap();
        std::fs::write(
            directory.join(format!("{capture}-{sequence}-stderr.txt")),
            &output.stderr,
        )
        .unwrap();
    }
    String::from_utf8(output.stdout).unwrap()
}

/// Calls actual696d0/6a994/a8fe0/697f0, preserving original clear/mask packets.
/// `Q` samples authored endpoints; `R` tests the low-level rectangle service.
pub(crate) fn original_clips(requests: &[(char, [f32; 4], bool)]) -> Vec<OriginalClip> {
    let mut input = String::new();
    for &(operation, values, equal) in requests {
        assert!(matches!(operation, 'Q' | 'R'));
        writeln!(
            input,
            "{operation} {} {} {} {} {}",
            values[0].to_bits(),
            values[1].to_bits(),
            values[2].to_bits(),
            values[3].to_bits(),
            u32::from(equal)
        )
        .unwrap();
    }
    let output = run(
        "SHIN_HIGURASHI_NATIVE_LEAF_CLIP_CLEAR",
        input,
        true,
        "masks",
    );
    assert_eq!(output.lines().count(), requests.len());
    output
        .lines()
        .zip(requests)
        .enumerate()
        .map(|(index, (line, &(operation, values, equal)))| {
            let mut tokens = line.split_whitespace();
            assert_eq!(tokens.next(), Some("G"));
            assert_eq!(word(&mut tokens), index as u32);
            assert_eq!(tokens.next(), Some("R"));
            let rect = words::<4>(&mut tokens);
            let clip = if operation == 'Q' {
                Clip::from_endpoints(values)
            } else {
                Clip(values)
            };
            assert_eq!(rect, clip.0.map(f32::to_bits));
            let mut draws = vec![];
            let mut depth = vec![];
            let mut stencil = vec![];
            let mut active = None;
            loop {
                match tokens.next().unwrap() {
                    "Y" => depth.push(words::<2>(&mut tokens)),
                    "S" => stencil.push(words::<5>(&mut tokens)),
                    "D" => draws.push(gpu::Draw::from_clip_packet(words(&mut tokens))),
                    "A" => active = Some(word(&mut tokens) != 0),
                    "E" => {
                        assert_eq!(word(&mut tokens) as usize, draws.len());
                        break;
                    }
                    unknown => panic!("Unreviewed original leaf mask event {unknown}"),
                }
            }
            assert!(tokens.next().is_none());
            let active = active.unwrap();
            assert_eq!(active, clip.active());
            assert_eq!(draws.len(), if active { 2 } else { 0 });
            if active {
                assert_eq!((draws[0].program, draws[1].program), (0, 1));
                assert_eq!(
                    draws[0].vertices,
                    [-1.0f32, 1.0, 0.0, 3.0, 1.0, 0.0, -1.0, -3.0, 0.0].map(f32::to_bits)
                );
            }
            let material_depth = depth[depth.len() - 2];
            let material_stencil = stencil[stencil.len() - 2];
            assert_eq!(material_depth, [if active { 1 } else { 7 }, 0]);
            assert_eq!(
                material_stencil,
                [if equal { 6 } else { 4 }, 0, 0, 2, 0x7ffff]
            );
            OriginalClip {
                draws,
                material_depth,
                material_stencil,
            }
        })
        .collect()
}

/// Narrow sibling seam for live Movie/animation/Blur call-site comparisons.
#[allow(
    dead_code,
    reason = "Retain the native mask seam for Movie/animation/Blur sibling comparisons"
)]
pub(crate) fn original_masks(endpoints: &[[f32; 4]], mask: u32) -> Vec<Vec<gpu::Draw>> {
    assert!(mask <= 1);
    original_clips(
        &endpoints
            .iter()
            .map(|&e| ('Q', e, mask != 0))
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .map(|clip| clip.draws)
    .collect()
}

#[derive(Clone)]
struct Profile {
    name: &'static str,
    operation: char,
    clips: [[f32; 4]; 2],
    parent: HigurashiTransform,
    reduced: bool,
}
fn profiles() -> Vec<Profile> {
    let standard = [160., 90., 1120., 630.];
    let mut profiles: Vec<_> = [
        ("full-sentinel", 'Q', [0., 0., 1280., 720.]),
        ("center", 'Q', standard),
        ("reversed", 'Q', [1120., 630., 160., 90.]),
        ("zero-width", 'Q', [640., 0., 640., 720.]),
        ("zero-height", 'Q', [0., 360., 1280., 360.]),
        ("exterior", 'Q', [1500., 0., 1800., 720.]),
        ("raw-negative-extent", 'R', [1040., 600., -800., -500.]),
        ("thin-left", 'Q', [0., 0., 1., 720.]),
        ("exterior-partial", 'Q', [-240., -180., 720., 420.]),
    ]
    .into_iter()
    .map(|(name, operation, clip)| Profile {
        name,
        operation,
        clips: [clip; 2],
        parent: HigurashiTransform::default(),
        reduced: false,
    })
    .collect();
    profiles.push(Profile {
        name: "consecutive-different",
        operation: 'Q',
        clips: [[0., 0., 610., 720.], [660., 60., 1280., 660.]],
        parent: HigurashiTransform::default(),
        reduced: false,
    });
    profiles.push(Profile {
        name: "parent-rotation",
        operation: 'Q',
        clips: [standard; 2],
        parent: HigurashiTransform {
            transform: native_rotation_z(0.07 * std::f32::consts::TAU),
            camera: Vec3::ZERO,
            inherited: vec2(34., -12.),
            wobble: vec2(9., -7.),
        },
        reduced: false,
    });
    profiles.push(Profile {
        name: "reduced-viewport",
        operation: 'Q',
        clips: [standard; 2],
        parent: HigurashiTransform::default(),
        reduced: true,
    });
    profiles
}
fn properties(endpoints: [f32; 4]) -> HigurashiProperties {
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    properties.tweener_mut(0).unwrap().fast_forward_to(-640.);
    properties.tweener_mut(1).unwrap().fast_forward_to(720.);
    for (i, value) in endpoints.into_iter().enumerate() {
        properties
            .tweener_mut(44 + i)
            .unwrap()
            .fast_forward_to(value);
    }
    properties
}
fn tile(index: usize) -> Tile {
    let colors = [
        vec4(0.91, 0.15, 0.23, 1.),
        vec4(0.17, 0.82, 0.28, 1.),
        vec4(0.12, 0.29, 0.93, 1.),
        vec4(0.89, 0.76, 0.18, 1.),
    ];
    let rects = [
        vec4(0., 0., 503., 287.),
        vec4(503., 0., 777., 287.),
        vec4(0., 287., 503., 433.),
        vec4(503., 287., 777., 433.),
    ];
    Tile {
        color: FloatColor4::from_vec4(colors[index]),
        rect: rects[index],
    }
}
fn original_tiles(profiles: &[Profile]) -> Vec<[gpu::Draw; 4]> {
    let mut input = String::new();
    for profile in profiles {
        for index in 0..4 {
            let tile = tile(index);
            let p = properties(profile.clips[index % 2]);
            write!(input, "T").unwrap();
            for value in p.values() {
                write!(input, " {:08x}", value.to_bits()).unwrap();
            }
            for seed in [0u32; 4] {
                write!(input, " {seed:08x}").unwrap();
            }
            for value in [0.; 4]
                .into_iter()
                .chain(profile.parent.transform.to_cols_array())
                .chain(profile.parent.camera.to_array())
                .chain(profile.parent.inherited.to_array())
                .chain(profile.parent.wobble.to_array())
                .chain(tile.color.into_array())
                .chain(tile.rect.to_array())
            {
                write!(input, " {:08x}", value.to_bits()).unwrap();
            }
            input.push_str(" 1\n");
        }
    }
    let output = run("SHIN_HIGURASHI_NATIVE_TILE", input, false, "tiles");
    assert_eq!(output.lines().count(), profiles.len() * 4);
    let draws: Vec<_> = output
        .lines()
        .map(|line| {
            let mut tokens = line.split_whitespace();
            assert_eq!(tokens.next(), Some("T"));
            let row: Vec<u32> = tokens
                .map(|w| u32::from_str_radix(w, 16).unwrap())
                .collect();
            assert_eq!(&row[..3], &[1, 1, 1]);
            assert_eq!(row.len(), 37);
            assert_eq!(&row[35..], &[1, 1]);
            gpu::Draw {
                program: 1,
                blend: row[35],
                primitive: 1,
                depth: [7, 0],
                stencil: [4, 0, 0, 2, 0x7ffff],
                uniforms: vec![row[19..35].to_vec()],
                vertices: row[3..19].to_vec(),
                count: 4,
                start: 0,
            }
        })
        .collect();
    draws
        .chunks_exact(4)
        .map(|draws| draws.to_vec().try_into().unwrap())
        .collect()
}

pub(crate) fn initialize(context: &mut PreRenderContext, target: &wgpu::TextureView, stencil: u32) {
    // Common test-fixture initialization only. Original a8fe0 is replayed as a
    // shader draw for every mask clear; attachment Clear is never used for it.
    let _pass = context
        .encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            multiview_mask: None,
            label: Some("Leaf clipping comparison fixture background"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.18,
                        g: 0.32,
                        b: 0.53,
                        a: 0.41,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: context.depth_stencil.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(stencil),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
}
fn exact(actual: &[u8], original: &[u8], context: &str) {
    let differences = actual.iter().zip(original).filter(|(a, b)| a != b).count();
    let max = actual
        .iter()
        .zip(original)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert_eq!(actual.len(), original.len());
    assert!(
        differences == 0,
        "{context}: {differences} different channels, maximum {max}"
    );
}

#[test]
#[ignore = "literal original clear/mask/Tile packets at720p/1080p; pure offscreen GPU, no audio/windows/user saves"]
fn higurashi_leaf_clip_depth_stencil_and_tile_pixels_match_original() {
    let (device, queue, _) = reference_gpu_assets();
    let profiles = profiles();
    let tiles = original_tiles(&profiles);
    let requests: Vec<_> = profiles
        .iter()
        .flat_map(|p| {
            [false, true]
                .into_iter()
                .flat_map(move |equal| p.clips.map(move |c| (p.operation, c, equal)))
        })
        .collect();
    let masks = original_clips(&requests);
    let mut renderer = gpu::Renderer::new(&device);
    let mut frames = 0;
    let mut call_site_frames = 0;
    let mut visible = 0;
    for (i, profile) in profiles.iter().enumerate() {
        for extent in [(1280, 720), (1920, 1080)] {
            let viewport =
                profile
                    .reduced
                    .then_some([0., 0., (extent.0 / 2) as f32, (extent.1 / 2) as f32]);
            let mut accepted = None;
            for (equal, initial_stencil) in [(false, 0), (false, 7), (true, 7)] {
                let originals =
                    &masks[i * 4 + usize::from(equal) * 2..i * 4 + usize::from(equal) * 2 + 2];
                let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    initialize(ctx, target, initial_stencil);
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Actual native clip + Tile packet",
                    );
                    if let Some([_, _, width, height]) = viewport {
                        pass.set_viewport(width, height);
                    }
                    for index in 0..4 {
                        let p = properties(profile.clips[index % 2]);
                        let clip = if profile.operation == 'Q' {
                            Clip::from_properties(&p)
                        } else {
                            Clip(profile.clips[index % 2])
                        };
                        let clipped = clip.prepare(&mut pass);
                        let tile = tile(index);
                        if equal {
                            let vertices = tile.vertices(&p.drawable_params());
                            pass.run(
                                RenderRequestBuilder::new()
                                    .depth_stencil_shorthand(7, true, clipped)
                                    .color_blend_type(ColorBlendType::Opaque)
                                    .build(
                                        RenderProgramWithArguments::Fill {
                                            vertices: VertexSource::VertexData {
                                                vertices: &vertices,
                                            },
                                            transform: p
                                                .composed_transform(&profile.parent)
                                                .compute_tile_transform(),
                                        },
                                        DrawPrimitive::TrianglesStrip,
                                    ),
                            );
                        } else {
                            tile.render_clipped(
                                &mut pass,
                                &p.drawable_params(),
                                p.composed_transform(&profile.parent)
                                    .compute_tile_transform(),
                                7,
                                PassKind::Opaque,
                                clipped,
                            );
                        }
                    }
                });
                let expected =
                    reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                        initialize(ctx, target, initial_stencil);
                        for (index, original) in tiles[i].iter().enumerate() {
                            let mask = &originals[index % 2];
                            for draw in &mask.draws {
                                renderer.draw(ctx, target, draw, None, viewport);
                            }
                            let mut draw = original.clone();
                            draw.depth = mask.material_depth;
                            draw.stencil = mask.material_stencil;
                            renderer.draw(ctx, target, &draw, None, viewport);
                        }
                    });
                exact(
                    &actual,
                    &expected,
                    &format!(
                        "{}/{extent:?}/equal{equal}/stencil{initial_stencil}",
                        profile.name
                    ),
                );
                let colors = actual
                    .chunks_exact(4)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                if !equal && initial_stencil == 7 {
                    assert_eq!(colors, 1, "strict equality must reject material");
                }
                if !equal && initial_stencil == 0 {
                    if !matches!(profile.name, "zero-width" | "zero-height" | "exterior") {
                        assert!(colors > 1, "{} has no visible mask effect", profile.name);
                        visible += 1;
                    }
                    accepted = Some(actual.clone());
                    if profile.operation == 'Q' {
                        let owner =
                            reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                                initialize(ctx, target, 0);
                                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                                    TextureTarget {
                                        view: target,
                                        kind: TextureTargetKind::Screen,
                                    },
                                    ctx.depth_stencil,
                                    "Actual leaf owner clip call site",
                                );
                                if let Some([_, _, width, height]) = viewport {
                                    pass.set_viewport(width, height);
                                }
                                for index in 0..4 {
                                    let tile = tile(index);
                                    let mut owner =
                                        HigurashiUserLayer::tile(tile.color, tile.rect, [0; 4]);
                                    owner.properties = properties(profile.clips[index % 2]);
                                    owner
                                        .render(&mut pass, &profile.parent, 7, PassKind::Opaque)
                                        .unwrap();
                                }
                            });
                        exact(
                            &owner,
                            &expected,
                            &format!("{} actual owner call site {extent:?}", profile.name),
                        );
                        call_site_frames += 1;
                    }
                }
                if equal {
                    exact(
                        &actual,
                        accepted.as_ref().unwrap(),
                        "inclusive equality equals passing strict draw",
                    );
                }
                frames += 1;
            }
        }
    }
    assert_eq!((frames, call_site_frames, visible), (72, 22, 18));
    eprintln!(
        "Original leaf clear/mask depth+stencil/Tile: {frames} byte-exact full720p/1080p frames; {call_site_frames} actual owner call sites; strict/inclusive/consecutive/reversed/empty/exterior/rotated/reduced viewport, no audio/windows/user saves"
    );
}

fn original_blocks(kind: u32, block: &crate::asset::picture::GpuPictureBlock) -> Vec<gpu::Draw> {
    let mut input = String::new();
    let mut values = INITIAL_VALUES;
    values[0] = -640;
    values[1] = 720;
    values[9] = 650;
    for opaque in [true, false] {
        write!(
            input,
            "R {kind} {} {} {} {}",
            u32::from(opaque),
            block.native_format,
            block.opaque_rect_count * 6,
            block.transparent_rect_count * 6
        )
        .unwrap();
        for value in values {
            write!(input, " {value}").unwrap();
        }
        input.push('\n');
    }
    let output = run(
        "SHIN_HIGURASHI_NATIVE_BASIC_FRAGMENTS",
        input,
        true,
        &format!("basic-kind{kind}"),
    );
    assert_eq!(output.lines().count(), 2);
    let mut draws = vec![];
    for line in output.lines() {
        let mut tokens = line.split_whitespace();
        assert_eq!(tokens.next(), Some("G"));
        let _ = word(&mut tokens);
        loop {
            match tokens.next().unwrap() {
                "R" => {
                    let _ = words::<5>(&mut tokens);
                }
                "M" | "Y" => {
                    let _ = words::<2>(&mut tokens);
                }
                "W" | "B" => {
                    let _ = words::<3>(&mut tokens);
                }
                "P" => {
                    let _ = words::<30>(&mut tokens);
                }
                "V" => {
                    let _ = word(&mut tokens);
                }
                "S" => {
                    let _ = words::<5>(&mut tokens);
                }
                "D" => {
                    let d = words::<86>(&mut tokens);
                    assert_eq!((d[0], d[4]), (kind, 0));
                    draws.push(gpu::Draw {
                        program: d[2],
                        blend: d[3],
                        primitive: d[4],
                        depth: d[11..13].try_into().unwrap(),
                        stencil: d[13..18].try_into().unwrap(),
                        uniforms: vec![d[22..38].to_vec(), d[38..42].to_vec(), d[54..58].to_vec()],
                        vertices: vec![],
                        count: d[5],
                        start: d[6],
                    });
                }
                "E" => {
                    let _ = words::<5>(&mut tokens);
                    assert!(tokens.next().is_none());
                    break;
                }
                unknown => panic!("Unreviewed original basic event {unknown}"),
            }
        }
    }
    assert!(!draws.is_empty());
    draws
}

#[test]
#[ignore = "real supplied PIC/BUP buffers + literal original mask/ordinary GLSL at720p/1080p; pure offscreen GPU, no audio/windows/user saves"]
fn higurashi_leaf_clip_real_picture_bustup_pixels_match_original() {
    let (device, queue, server) = reference_gpu_assets();
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let info = &scenario.picture_info[0];
    let picture: Arc<Picture> = server
        .load_sync(format!(
            "/picture/{}.pic",
            selected_asset_name(info.name.as_str(), 0)
        ))
        .unwrap();
    let info = &scenario.bustup_info[0];
    let bustup: Arc<HigurashiBustup> = shin_tasks::block_on(server.load_with_args(
        format!("/bustup/{}.bup", selected_asset_name(info.name.as_str(), 0)),
        BustupArgs {
            expression: info.emotion.as_str().to_owned(),
            character_id: CharacterId::from_metadata(info.lipsync_character_id),
            disable_animations: false,
        },
    ))
    .unwrap();
    let source = [
        (2, picture.blocks.iter_placements().next().unwrap().2),
        (
            3,
            bustup
                .body
                .blocks
                .iter()
                .flatten()
                .find(|b| b.transparent_rect_count > 0)
                .unwrap()
                .as_ref(),
        ),
    ];
    let endpoints = [
        [0., 0., 1280., 720.],
        [160., 90., 1120., 630.],
        [1120., 630., 160., 90.],
        [640., 0., 640., 720.],
        [1500., 0., 1800., 720.],
        [0., 0., 720., 540.],
    ];
    let masks = original_clips(&endpoints.map(|e| ('Q', e, false)));
    let mut renderer = gpu::Renderer::new(&device);
    let mut frames = 0;
    for (kind, source) in source {
        let originals = original_blocks(kind, source);
        let native_matrix = Mat4::from_cols_array(
            &originals[0].uniforms[0]
                .iter()
                .copied()
                .map(f32::from_bits)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        );
        for (index, (endpoints, mask)) in endpoints.into_iter().zip(&masks).enumerate() {
            for extent in [(1280, 720), (1920, 1080)] {
                let viewport =
                    (index == 5).then_some([0., 0., (extent.0 / 2) as f32, (extent.1 / 2) as f32]);
                let mut p = properties(endpoints);
                p.tweener_mut(9).unwrap().fast_forward_to(650.);
                let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    initialize(ctx, target, 0);
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Actual clipped native PIC/BUP Core",
                    );
                    if let Some([_, _, width, height]) = viewport {
                        pass.set_viewport(width, height);
                    }
                    for pass_kind in [PassKind::Opaque, PassKind::Transparent] {
                        if let Some(params) =
                            PictureBlockParams::setup(pass_kind, &p.drawable_params())
                        {
                            let clipped = Clip::from_properties(&p).prepare(&mut pass);
                            block::render_native(
                                source,
                                &mut pass,
                                RenderRequestBuilder::new()
                                    .depth_stencil_shorthand(7, false, clipped),
                                params,
                                native_matrix,
                            );
                        }
                    }
                });
                let expected =
                    reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                        initialize(ctx, target, 0);
                        // Alpha650 selects one transparent owner pass. Native
                        // Clip is prepared once before all format2 subranges.
                        for clear in &mask.draws {
                            renderer.draw(ctx, target, clear, None, viewport);
                        }
                        for draw in &originals {
                            let mut draw = draw.clone();
                            draw.depth = mask.material_depth;
                            draw.stencil = mask.material_stencil;
                            renderer.draw_block(ctx, target, &draw, source, viewport);
                        }
                    });
                exact(
                    &actual,
                    &expected,
                    &format!("kind{kind}/clip{index}/{extent:?}"),
                );
                if index == 0 {
                    assert!(
                        actual
                            .chunks_exact(4)
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            > 3,
                        "real source has no spatial detail"
                    );
                }
                if matches!(index, 3 | 4) {
                    assert_eq!(
                        actual
                            .chunks_exact(4)
                            .collect::<std::collections::BTreeSet<_>>()
                            .len(),
                        1,
                        "empty/exterior clipping must reject every source sample"
                    );
                }
                frames += 1;
            }
        }
    }
    assert_eq!(frames, 24);
    eprintln!(
        "Original leaf masks + real PIC/BUP Core: {frames} byte-exact full720p/1080p frames; partial alpha/nonblack background/native raw matrix+asset bounds+indexed buffers/reversed/zero/exterior/reduced viewport; no audio/windows/user saves"
    );
}
