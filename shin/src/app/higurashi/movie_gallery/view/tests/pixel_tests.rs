//! Original uploaded parent packets + literal primary shaders, actual TXA/font.
//! No production packet plan constructs the independent expected frame.
use super::*;
use crate::app::higurashi::native_shader_replay::{self as replay, ShaderSet};

fn clear(ctx: &mut crate::render::PreRenderContext, target: &wgpu::TextureView) {
    let mut pass = ctx.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            view: target,
            kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
        },
        Some(ctx.depth_stencil),
        "Movie independent nonblack baseline",
    );
    pass.clear(
        Some(shin_core::primitives::color::UnormColor::from_rgba(
            31, 63, 127, 193,
        )),
        Some(0),
        Some(1.),
    );
}

#[test]
#[ignore = "64 Movie/systex/NewRodin720p1080p parent frames incl4 explicitly synthetic scrollbar frames against original uploaded packets/literal primary2/3/4/5; no audio/window/saves"]
fn higurashi_movie_real_assets_parent_pixels_match_literal_native_shaders() {
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(evidence().join("profiles.json")).unwrap()).unwrap();
    let (originals, names) = parse(&reexecute());
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = HigurashiScenario::new(
        std::fs::read(required("SHIN_HIGURASHI_MOVIE_GALLERY_RENDER_SCENARIO"))
            .unwrap()
            .into(),
    )
    .unwrap();
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync(super::super::super::ATLAS_PATH).unwrap();
    let system: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();
    View::validate_atlas(&atlas).unwrap();
    let points = originals
        .iter()
        .flat_map(|o| &o.draws)
        .filter(|d| d.header[2] == 2)
        .map(|d| d.header[3])
        .collect::<Vec<_>>();
    let glyphs = points
        .iter()
        .copied()
        .zip(
            font.clone().load_glyphs(
                device.clone(),
                queue.clone(),
                &points
                    .iter()
                    .map(|&p| char::from_u32(p).unwrap())
                    .collect::<Vec<_>>(),
            ),
        )
        .collect::<HashMap<_, _>>();
    let mut view = View::new(font.clone());
    let mut renderer = replay::Renderer::new(&device, ShaderSet::Chart);
    // Authored roster/lock/hint/close/focus cases and TWO explicitly synthetic
    // scrollbar profiles; both physical extents. Native reexecution precedes GPU.
    // Capture-bearing First overlay pixels are a separate owner-level concern.
    let selected = [
        0, 1, 5, 11, 12, 23, 24, 35, 36, 47, 48, 59, 60, 71, 72, 83, 84, 95, 96, 97, 98, 99, 100,
        103, 107, 111, 140, 143, 148, 166, 171, 181,
    ];
    let mut frames = 0;
    let mut synthetic_frames = 0;
    for i in selected {
        let p = &manifest.profiles[i];
        let n = &originals[i];
        assert_eq!(p.index, i);
        assert_eq!(p.capture, 0);
        assert!(n.draws.iter().all(|d| d.header[2] < 3));
        let state = prepared_state(p, &scenario);
        view.synchronize(&server, &state).unwrap();
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    let mut pass = ctx.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget { view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture },
                    Some(ctx.depth_stencil),"Movie retained production parent",
                );
                    view.render(
                        &state,
                        if p.phase != 0 { Some(&atlas) } else { None },
                        &system,
                        p.layout != 0,
                        &mut pass,
                    )
                    .unwrap();
                },
            );
            let original = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    for d in &n.draws {
                        let source = match d.header[2] {
                            0 => atlas.get(&names[&(0, d.header[3])]).as_source(),
                            1 => system.get(&names[&(1, d.header[3])]).as_source(),
                            2 => glyphs[&d.header[3]].as_texture_source().unwrap(),
                            _ => unreachable!(),
                        };
                        let uniforms = match d.header[0] {
                            2 => vec![d.matrix.to_vec()],
                            3 => vec![
                                d.matrix.to_vec(),
                                d.uniforms[0][..4].to_vec(),
                                d.uniforms[1][..4].to_vec(),
                            ],
                            4 => vec![
                                d.matrix.to_vec(),
                                d.uniforms[0][..16].to_vec(),
                                d.uniforms[1][..4].to_vec(),
                            ],
                            5 => vec![d.matrix.to_vec(), d.uniforms[0][..4].to_vec()],
                            _ => unreachable!(),
                        };
                        let draw = replay::Draw {
                            program: d.header[0],
                            blend: d.header[1],
                            primitive: 1,
                            depth: [7, 0],
                            stencil: [7, 0, 0, 0, 255],
                            uniforms,
                            vertices: d.vertices.clone(),
                            count: d.header[7],
                            start: 0,
                        };
                        renderer.draw(ctx, target, &draw, Some(source), None);
                    }
                },
            );
            assert_eq!(actual.len(), original.len());
            let mismatched = actual.iter().zip(&original).filter(|(a, b)| a != b).count();
            let max_error = actual
                .iter()
                .zip(&original)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert_eq!(
                mismatched, 0,
                "Movie profile{i} {} {extent:?}: {mismatched} channels differ/max{max_error}",
                p.name
            );
            assert_eq!(
                actual.chunks_exact(4).any(|p| p != [31, 63, 127, 193]),
                p.phase != 0
            );
            frames += 1;
            synthetic_frames += usize::from(p.synthetic_scrollbar_height);
        }
    }
    assert_eq!(frames, 64);
    assert_eq!(synthetic_frames, 4);
    println!(
        "Movie:64 exact full720p1080p parent frames,actual TXA/systex/NewRodin,original native packets/literal primary2/3/4/5;one retained text owner;no audio/window/save"
    );
}
