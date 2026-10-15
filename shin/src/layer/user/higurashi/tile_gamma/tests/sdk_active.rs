//! Complete original SDK packets for finite active Gamma. Production admission
//! now uses the actual owner for the finite active clip/packed-color transport.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::user::higurashi::leaf_clip::gpu_tests::{gpu, initialize},
};
use shin_render::{
    DepthFunction, DepthState, DepthStencilState, StencilFunction, StencilPipelineState,
    StencilState,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};

fn directory() -> PathBuf {
    root().join("reverse-engineering/higurashi/tile-gamma-active-clip-20261014/allocation-closure/whole-tile-domains")
}
fn words(v: &serde_json::Value) -> Vec<u32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap().try_into().unwrap())
        .collect()
}
pub(super) fn draw(v: &serde_json::Value) -> gpu::Draw {
    gpu::Draw {
        program: v["program"].as_u64().unwrap() as u32,
        blend: v["blend"].as_u64().unwrap() as u32,
        primitive: v["primitive"].as_u64().unwrap() as u32,
        depth: words(&v["depth"]).try_into().unwrap(),
        stencil: words(&v["stencil"]).try_into().unwrap(),
        uniforms: v["uniforms"]
            .as_array()
            .unwrap()
            .iter()
            .map(words)
            .collect(),
        vertices: words(&v["vertices"]),
        count: v["count"].as_u64().unwrap() as u32,
        start: v["start"].as_u64().unwrap() as u32,
    }
}
fn inputs(row: &serde_json::Value) -> (Case, FullCanvas, bool) {
    let w = words(&row["input"]);
    assert_eq!(w.len(), 109);
    let v: Vec<f32> = w[5..].iter().copied().map(f32::from_bits).collect();
    let raw: [f32; 65] = v[..65].try_into().unwrap();
    let mut properties = HigurashiProperties::from_seeds(w[70..74].try_into().unwrap());
    for (index, value) in raw.into_iter().enumerate() {
        properties
            .tweener_mut(index)
            .unwrap()
            .fast_forward_to(value);
    }
    for i in 0..4 {
        properties.oscillators[i].phase = v[69 + i];
    }
    let parent = HigurashiTransform {
        transform: Mat4::from_cols_array(v[73..89].try_into().unwrap()),
        camera: Vec3::from_slice(&v[89..92]),
        inherited: Vec2::from_slice(&v[92..94]),
        wobble: Vec2::from_slice(&v[94..96]),
    };
    let tile = Tile {
        color: FloatColor4::from_vec4(Vec4::from_slice(&v[96..100])),
        rect: Vec4::from_slice(&v[100..104]),
    };
    let requested_pass = if w[4] == 1 {
        PassKind::Opaque
    } else {
        PassKind::Transparent
    };
    // Reuse the production packed-color/material gate, retaining its current
    // fullcover admission. The candidate below separately prepares active
    // clipping. This test seam does not widen production's clip guard.
    let full = [0., 0., 1280., 720.];
    let gamma = FullCanvas::new(
        &tile,
        &properties.drawable_params(),
        full,
        Clip::from_endpoints(full),
    )
    .unwrap();
    (
        Case {
            index: row["index"].as_u64().unwrap() as usize,
            raw,
            properties,
            parent,
            tile,
            requested_pass,
            original: None,
        },
        gamma,
        w[3] != 0,
    )
}
fn vertices_words(gamma: &FullCanvas) -> Vec<u32> {
    let vertices = gamma.vertices();
    let bytes: &[u8] = bytemuck::cast_slice(&vertices);
    let mut words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    // Only the native color word is BGRA; geometry floats stay unchanged.
    for i in (3..16).step_by(4) {
        words[i] = bgra(words[i]);
    }
    words
}
fn verify(row: &serde_json::Value) {
    let (case, gamma, flip) = inputs(row);
    let original = draw(&row["draws"][2]);
    let active = super::super::ActiveClip::new(
        &case.tile,
        &case.properties.drawable_params(),
        case.raw[44..48].try_into().unwrap(),
    )
    .unwrap();
    assert_eq!(vertices_words(&active.material), vertices_words(&gamma));
    assert_eq!(
        active.clip.0.map(f32::to_bits),
        Clip::from_properties(&case.properties).0.map(f32::to_bits)
    );
    assert_eq!(
        vertices_words(&gamma),
        original.vertices,
        "case{} vertices",
        case.index
    );
    assert_eq!(
        native_blend(gamma.color_blend_type()),
        original.blend,
        "case{} preGamma blend",
        case.index
    );
    assert_eq!(gamma.pass_kind(), case.requested_pass);
    let mut matrix = case
        .properties
        .composed_transform(&case.parent)
        .compute_native_tile_transform()
        .to_cols_array()
        .map(f32::to_bits);
    if flip {
        for i in (1..16).step_by(4) {
            matrix[i] ^= 0x8000_0000;
        }
    }
    assert_eq!(
        matrix.as_slice(),
        original.uniforms[0],
        "case{} actual bound matrix",
        case.index
    );
    let mask = draw(&row["draws"][1]);
    let clip = Clip::from_properties(&case.properties);
    assert!(clip.active());
    let clip_vertices = clip.vertices();
    let vertex_bytes: &[u8] = bytemuck::cast_slice(&clip_vertices);
    let vertices: Vec<u32> = vertex_bytes
        .chunks_exact(4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    assert_eq!(vertices, mask.vertices, "case{} clip vertices", case.index);
    let mut projection = Clip::native_projection().to_cols_array().map(f32::to_bits);
    if flip {
        for i in (1..16).step_by(4) {
            projection[i] ^= 0x8000_0000;
        }
    }
    assert_eq!(
        projection.as_slice(),
        mask.uniforms[0],
        "case{} bound mask matrix",
        case.index
    );
}
#[test]
fn higurashi_tile_gamma_sdk_active_packets_match_port_geometry_and_transforms() {
    let rows: serde_json::Value =
        serde_json::from_slice(&fs::read(directory().join("decoded-draws.json")).unwrap()).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2256);
    for row in rows {
        verify(row);
    }
    println!(
        "TileGamma:2256 complete SDK active profiles; independent port vertices, parent/property transforms, preGamma blend/pass and actual clip/flip matrices match raw decoded packets"
    );
}
fn candidate(pass: &mut RenderPass, case: &Case, gamma: &FullCanvas, probe: bool) {
    // Exercise the actual owner dispatch rather than constructing the helper
    // with substituted fullcover endpoints. Original packet replay stays raw.
    let layer = super::owner::owner(case);
    layer
        .render(pass, &case.parent, 7, case.requested_pass)
        .unwrap();
    let matrix = case
        .properties
        .composed_transform(&case.parent)
        .compute_tile_transform();
    let mut vertices = gamma.vertices();
    if probe {
        // Declared diagnostic draw exposes the stencil footprint even when
        // Gamma's zero alpha makes a transparent material leave RGB unchanged.
        for vertex in &mut vertices {
            vertex.color = UnormColor(0xff40_80ff);
        }
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Opaque)
                .depth_stencil(DepthStencilState {
                    depth: DepthState {
                        function: DepthFunction::Always,
                        write_enable: false,
                    },
                    stencil: StencilState {
                        pipeline: StencilPipelineState {
                            function: StencilFunction::Equal,
                            ..Default::default()
                        },
                        stencil_reference: 7,
                    },
                })
                .build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        transform: matrix,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
#[test]
#[ignore = "288 full720p/1080p original SDK packet/literal GLSL comparisons including observable stencil probes; pure GPU, no audio/window/save"]
fn higurashi_tile_gamma_sdk_active_pixels_match_literal_original_commands() {
    let rows: serde_json::Value =
        serde_json::from_slice(&fs::read(directory().join("gpu-cases.json")).unwrap()).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 72);
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let mut frames = 0;
    let mut visible_probe_frames = 0;
    let mut visible_by_blend = [0usize; 5];
    for row in rows {
        verify(row);
        let (case, gamma, flip) = inputs(row);
        assert!(!flip);
        let draws: Vec<_> = row["draws"].as_array().unwrap().iter().map(draw).collect();
        for extent in [(1280, 720), (1920, 1080)] {
            let mut natural: Option<Vec<u8>> = None;
            for probe in [false, true] {
                let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    initialize(ctx, target, 0);
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Candidate active TileGamma with original SDK oracle",
                    );
                    candidate(&mut pass, &case, &gamma, probe);
                });
                let expected =
                    reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                        initialize(ctx, target, 0);
                        for d in &draws {
                            renderer.draw(ctx, target, d, None, None);
                        }
                        if probe {
                            let mut diagnostic = draws[2].clone();
                            diagnostic.blend = 1;
                            diagnostic.depth = [7, 0];
                            diagnostic.stencil = [2, 0, 0, 0, 0x7ffff];
                            for i in (3..16).step_by(4) {
                                diagnostic.vertices[i] = bgra(0xff40_80ff);
                            }
                            renderer.draw(ctx, target, &diagnostic, None, None);
                        }
                    });
                let differences = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
                assert_eq!(
                    differences, 0,
                    "case{}/{extent:?}/probe{probe}: differing channels",
                    case.index
                );
                if probe {
                    let baseline = natural.take().unwrap();
                    let changed = actual.iter().zip(&baseline).filter(|(a, b)| a != b).count();
                    println!(
                        "FRAME case={} source={} clip={} parent={} extent={extent:?} probe={probe} matching_channels={} changed_from_natural={changed}",
                        case.index,
                        row["source_index"],
                        row["clip_index"],
                        row["parent_index"],
                        actual.len()
                    );
                    // Perspective parent2 produces measured depth one float step below
                    //0.5 in case776. Empty mask geometry alone cannot reject that
                    //native Less test. Constant-W parents0/1 remain controls.
                    if row["clip_index"].as_u64().unwrap() == 4
                        && row["parent_index"].as_u64().unwrap() != 2
                    {
                        assert_eq!(
                            changed, 0,
                            "case{}/source{}/{extent:?}: zero-area clip admitted a stencil probe",
                            case.index, row["source_index"]
                        );
                    }
                    if changed > 0 {
                        visible_probe_frames += 1;
                        visible_by_blend[draws[2].blend as usize] += 1;
                    }
                } else {
                    natural = Some(actual);
                }
                frames += 1;
            }
        }
    }
    assert_eq!(frames, 288);
    assert!(
        visible_by_blend[1..].iter().all(|x| *x > 0),
        "missing positive stencil control perblend:{visible_by_blend:?}"
    );
    println!(
        "TileGamma:288 exact720p/1080p frame comparisons from complete original SDK commands;{visible_probe_frames} visible stencil diagnostic frames;native NVN hardware output not claimed"
    );
}

mod depth_diagnostic;
