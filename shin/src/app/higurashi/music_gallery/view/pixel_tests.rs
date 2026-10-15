//! Original uploaded parent packets + literal primary shaders, actual TXA/font.
//! No production packet plan constructs the independent expected frame.
use super::*;
#[path = "../../chart/view_tests/gpu.rs"]
mod gpu;

fn clear(ctx: &mut crate::render::PreRenderContext, target: &wgpu::TextureView) {
    let mut pass = ctx.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            view: target,
            kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
        },
        Some(ctx.depth_stencil),
        "Music independent nonblack baseline",
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
#[ignore = "64 actual Music/systex/NewRodin720p1080p parent frames against original uploaded packets/literal primary2/3/4/5; no audio/window/saves"]
fn higurashi_music_real_assets_parent_pixels_match_literal_native_shaders() {
    let manifest: Manifest = serde_json::from_slice(
        &std::fs::read(evidence().join("music-gallery-render-capture/profiles.json")).unwrap(),
    )
    .unwrap();
    let (originals, names) = parse(
        &std::fs::read_to_string(evidence().join("music-gallery-render-capture/output.txt"))
            .unwrap(),
    );
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = model_tests::scenario();
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
    let mut renderer = gpu::Renderer::new(&device);
    // Original authored title bands, both hint layouts, locked/partial unlocks,
    // fractional/end scrolling, absent art and row/close flash boundaries.
    // Capture-bearing First overlay pixels are a separate owner-level concern.
    let selected = [
        0, 1, 30, 31, 62, 63, 94, 95, 126, 127, 156, 157, 158, 170, 206, 218, 232, 253, 254, 255,
        259, 268, 282, 283, 284, 285, 287, 289, 298, 300, 301, 304,
    ];
    let mut frames = 0;
    for i in selected {
        let p = &manifest.profiles[i];
        let n = &originals[i];
        assert_eq!(p.index, i);
        assert_eq!(p.capture, 0);
        assert!(n.draws.iter().all(|d| d.header[2] < 3));
        let mut m = Model::new(
            &scenario.music_box_info,
            &model_tests::unlocks(p.unlock),
            p.append != 0,
        )
        .unwrap();
        m.scroll.fast_forward_to(p.scroll);
        m.selected = p.selected;
        m.playing = p.playing as i32;
        m.elapsed = p.elapsed;
        m.close_flash.fast_forward_to(p.close);
        m.scrollbar = Scrollbar {
            position: p.scroll as i32,
            visible: 818,
            first: 0,
            end: m.document_height,
        };
        for &(row, value) in &p.flashes {
            m.rows[row].flash.fast_forward_to(value);
        }
        view.synchronize(&server, &m).unwrap();
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    let mut pass = ctx.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget { view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture },
                    Some(ctx.depth_stencil),"Music retained production parent",
                );
                    view.render(
                        &m,
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
                        let draw = gpu::Draw {
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
            assert_eq!(
                actual, original,
                "Music profile{i} {} {extent:?} literal primary RGBA",
                p.name
            );
            assert_eq!(
                actual.chunks_exact(4).any(|p| p != [31, 63, 127, 193]),
                p.phase != 0
            );
            frames += 1;
        }
    }
    assert_eq!(frames, 64);
    println!(
        "Music:64 exact full720p1080p parent frames,actual TXA/systex/NewRodin,original native packets/literal primary2/3/4/5;one retained text owner;no audio/window/save"
    );
}
