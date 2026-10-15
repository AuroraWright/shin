//! Combined original mask transport and already-compared material packets.
//! These tests exercise actual live render entry points. They add no claim
//! about original SDK rasterization or the missing supplied Sakura texture.
use super::{
    Clip,
    gpu_tests::{
        self, OriginalClip,
        gpu::{Draw, Renderer},
    },
};
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::higurashi::HigurashiTransform,
        user::higurashi::{animation, effect, tile::Tile},
    },
};
use glam::{Vec4, vec4};
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    PassKind,
    gpu_texture::GpuTexture,
    shaders::types::{
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosTexVertex,
    },
};

pub(crate) fn properties(endpoints: [f32; 4]) -> HigurashiProperties {
    let mut p = HigurashiProperties::from_seeds([1, 2, 3, 4]);
    for (i, value) in endpoints.into_iter().enumerate() {
        p.tweener_mut(44 + i).unwrap().fast_forward_to(value);
    }
    p
}
pub(crate) fn vertices(vertices: &[PosTexVertex]) -> Vec<u32> {
    vertices
        .iter()
        .flat_map(|v| {
            [
                v.position.x,
                v.position.y,
                v.texture_position.x,
                v.texture_position.y,
            ]
            .map(f32::to_bits)
        })
        .collect()
}
pub(crate) fn ordinary(
    matrix: glam::Mat4,
    color: FloatColor4,
    param: Vec4,
    vertices: Vec<u32>,
    original: &OriginalClip,
) -> Draw {
    Draw {
        program: 6,
        blend: 2,
        primitive: 1,
        depth: original.material_depth,
        stencil: original.material_stencil,
        uniforms: vec![
            matrix.to_cols_array().map(f32::to_bits).to_vec(),
            color.into_array().map(f32::to_bits).to_vec(),
            param.to_array().map(f32::to_bits).to_vec(),
        ],
        vertices,
        count: 4,
        start: 0,
    }
}
pub(crate) fn exact(actual: &[u8], original: &[u8], label: &str) {
    assert_eq!(actual.len(), original.len());
    let differences = actual.iter().zip(original).filter(|(a, b)| a != b).count();
    assert_eq!(differences, 0, "{label}:different RGBA channels");
}

/// The five fixtures are full, partial, reversed-partial, zero and exterior.
/// Keep the reference observable: two equal empty images cannot certify a clip.
pub(crate) fn coverage(frames: &[Vec<u8>], label: &str) {
    assert_eq!(frames.len(), 5);
    assert!(
        frames[0] != frames[1],
        "{label}:partial must change visible full coverage"
    );
    assert!(
        frames[1] == frames[2],
        "{label}:reversed endpoints retain coverage"
    );
    assert!(
        frames[3] == frames[4],
        "{label}:zero and exterior reject all material"
    );
    assert!(
        frames[0] != frames[3],
        "{label}:full material must be visible"
    );
}

fn probe() -> Tile {
    Tile {
        color: FloatColor4::from_vec4(vec4(0.25, 0.8, 0.5, 1.)),
        rect: vec4(0., 0., 1280., 720.),
    }
}
fn probe_draw(tile: &Tile, p: &HigurashiProperties) -> Draw {
    Draw {
        program: 1,
        blend: 1,
        primitive: 1,
        depth: [1, 0],
        stencil: [4, 0, 0, 2, 0x8ffff],
        uniforms: vec![
            Clip::native_projection()
                .to_cols_array()
                .map(f32::to_bits)
                .to_vec(),
        ],
        vertices: tile
            .vertices(&p.drawable_params())
            .iter()
            .flat_map(|v| {
                // Original program1 consumes BGRA and swizzles in_COLOR0.zyxw;
                // the shared production vertex stores RGBA instead.
                let rgba=v.color.0;
                let bgra=(rgba & 0xff00ff00) | ((rgba & 255)<<16) | ((rgba>>16)&255);
                [
                    v.position.x.to_bits(),
                    v.position.y.to_bits(),
                    v.position.z.to_bits(),
                    bgra,
                ]
            })
            .collect(),
        count: 4,
        start: 0,
    }
}

#[test]
#[ignore = "actual Focus/Blur/controlled-empty Sakura clip call sites, independent original mask/material GLSL at720p/1080p; no audio/windows/saves"]
fn higurashi_animation_and_blur_clip_call_sites_match_original_shader_transport() {
    let (device, queue, server) = reference_gpu_assets();
    let atlas: std::sync::Arc<crate::adv::character_view::CharacterTextures> =
        server.load_sync("/effects.txa").unwrap();
    let endpoints = [
        [0., 0., 1280., 720.],
        [160., 90., 1120., 630.],
        [1120., 630., 160., 90.],
        [640., 0., 640., 720.],
        [1500., 0., 1800., 720.],
    ];
    let originals = gpu_tests::original_clips(&endpoints.map(|p| ('Q', p, true)));
    let blur_originals = gpu_tests::original_clips(&endpoints.map(|p| ('Q', p, false)));
    let mut renderer = Renderer::new(&device);
    let image: Vec<u8> = (0..48)
        .flat_map(|y| {
            (0..64).flat_map(move |x| [(x * 3) as u8, (y * 5) as u8, (x + y * 2) as u8, 255])
        })
        .collect();
    let source = GpuTexture::new_static_with_data(
        &device,
        &queue,
        Some("Clip Blur pattern"),
        (64, 48).into(),
        wgpu::TextureFormat::Rgba8Unorm,
        &image,
    );
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let mut focus_frames = Vec::new();
        for (case, &endpoints) in endpoints.iter().enumerate() {
            let mut p = properties(endpoints);
            let prepared =
                shin_tasks::block_on(animation::Prepared::load(&server, 8, &[0; 8])).unwrap();
            let mut focus = prepared.construct(&mut p, None, 37).unwrap().unwrap();
            focus.advance(Ticks::from_f32(3.), &mut p);
            let packets = focus
                .packets(&p, &Default::default(), PassKind::Transparent)
                .unwrap();
            assert_eq!(packets.len(), 40);
            let actual = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                gpu_tests::initialize(ctx, target, 0);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    ctx.depth_stencil,
                    "Actual Focus clip",
                );
                focus
                    .render(&mut pass, &p, &Default::default(), 7, PassKind::Transparent)
                    .unwrap();
            });
            let original = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                gpu_tests::initialize(ctx, target, 0);
                for draw in &originals[case].draws {
                    renderer.draw(ctx, target, draw, None, None);
                }
                for packet in &packets {
                    assert_eq!(packet.shader, shin_render::LayerFragmentShader::Default);
                    let r = packet.rect;
                    let uv = packet.uv;
                    let v = [
                        [r.x, r.y, uv.x, uv.y],
                        [r.z, r.y, uv.z, uv.y],
                        [r.x, r.w, uv.x, uv.w],
                        [r.z, r.w, uv.z, uv.w],
                    ]
                    .into_iter()
                    .flat_map(|v| v.map(f32::to_bits))
                    .collect();
                    let draw = ordinary(
                        packet.matrix,
                        packet.color,
                        packet.shader_param,
                        v,
                        &originals[case],
                    );
                    renderer.draw(
                        ctx,
                        target,
                        &draw,
                        Some(atlas.get(packet.texture).as_source()),
                        None,
                    );
                }
            });
            exact(&actual, &original, &format!("Focus{case}/{size:?}"));
            focus_frames.push(original);
            frames += 1;

            let p = properties(endpoints);
            let view = effect::Prepared::new(5).unwrap().clone_refresh();
            let packet = view.packet(&p, PassKind::Opaque).unwrap().unwrap();
            // Native Blur's verified positive final quad is offscreen. Keep
            // that packet and probe its required mask side effect separately.
            let probe = probe();
            let actual = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                gpu_tests::initialize(ctx, target, 0);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    ctx.depth_stencil,
                    "Actual Blur final clip",
                );
                view.render(&mut pass, source.as_source(), &p, 7, PassKind::Opaque)
                    .unwrap();
                probe.render_clipped(
                    &mut pass,
                    &p.drawable_params(),
                    HigurashiTransform::convert_clip_depth(Clip::native_projection()),
                    8,
                    PassKind::Opaque,
                    true,
                );
            });
            let original = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                gpu_tests::initialize(ctx, target, 0);
                for draw in &blur_originals[case].draws {
                    renderer.draw(ctx, target, draw, None, None);
                }
                let draw = Draw {
                    program: 23,
                    blend: 1,
                    primitive: 1,
                    depth: blur_originals[case].material_depth,
                    stencil: blur_originals[case].material_stencil,
                    uniforms: vec![
                        packet.projection.to_cols_array().map(f32::to_bits).to_vec(),
                        packet
                            .distances
                            .iter()
                            .flat_map(|d| d.to_array().map(f32::to_bits))
                            .collect(),
                        packet.color.to_array().map(f32::to_bits).to_vec(),
                        packet.weights.to_array().map(f32::to_bits).to_vec(),
                    ],
                    vertices: vertices(&packet.vertices),
                    count: 4,
                    start: 0,
                };
                renderer.draw(ctx, target, &draw, Some(source.as_source()), None);
                renderer.draw(ctx, target, &probe_draw(&probe, &p), None, None);
            });
            exact(&actual, &original, &format!("Blur{case}/{size:?}"));
            frames += 1;

            // Supplied atlas has no Sakura entry. A controlled empty native
            // vector isolates its required mask without inventing that asset.
            let mut p = properties(endpoints);
            let prepared =
                shin_tasks::block_on(animation::Prepared::load(&server, 4, &[0; 8])).unwrap();
            let mut sakura = prepared.construct(&mut p, None, 37).unwrap().unwrap();
            sakura.reference_clear_sakura();
            assert!(
                sakura
                    .packets(&p, &Default::default(), PassKind::Transparent)
                    .unwrap()
                    .is_empty()
            );
            let probe = crate::layer::user::higurashi::leaf_clip::call_site_tests::probe();
            let drawable = p.drawable_params();
            let actual = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                gpu_tests::initialize(ctx, target, 0);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    ctx.depth_stencil,
                    "Empty Sakura mask probe",
                );
                sakura
                    .render(&mut pass, &p, &Default::default(), 7, PassKind::Transparent)
                    .unwrap();
                probe.render_clipped(
                    &mut pass,
                    &drawable,
                    HigurashiTransform::convert_clip_depth(Clip::native_projection()),
                    8,
                    PassKind::Opaque,
                    true,
                );
            });
            let original = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                gpu_tests::initialize(ctx, target, 0);
                for draw in &originals[case].draws {
                    renderer.draw(ctx, target, draw, None, None);
                }
                let draw = probe_draw(&probe, &p);
                renderer.draw(ctx, target, &draw, None, None);
            });
            exact(&actual, &original, &format!("Empty Sakura{case}/{size:?}"));
            frames += 1;
        }
        coverage(&focus_frames, &format!("Focus/{size:?}"));
    }
    assert_eq!(frames, 30);
    eprintln!(
        "Live clipping call sites:30 exact720p/1080p Focus/Blur-mask-side-effect/controlled-empty Sakura frames;original masks/material GLSL;offscreen Blur quad and supplied Sakura asset gap preserved;no audio/windows/saves"
    );
}
