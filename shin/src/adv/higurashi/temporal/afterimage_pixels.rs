//! NEW independent original ba038/program6/blend5 pixel proof. The native
//! resource/clock/packet records come from the accepted frozen temporal corpus;
//! source pixels are an explicit controlled GPU-transport fixture. This does
//! not claim that the standalone packet test implements live Scene history.
use super::*;
use std::{collections::BTreeSet, path::PathBuf};

use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::user::higurashi::leaf_clip::gpu_tests::{gpu, initialize},
    render::PreRenderContext,
};
use shin_render::{
    gpu_texture::GpuTexture,
    shaders::types::texture::{TextureSource, TextureTarget, TextureTargetKind},
};

#[derive(Clone, Debug)]
pub(crate) enum ResourceEvent {
    Allocate {
        id: u32,
        kind: u32,
        extent: [u32; 2],
    },
    InitialClear {
        id: u32,
        color: u32,
    },
    Release {
        id: u32,
        remaining: u32,
    },
}
#[derive(Clone)]
pub(crate) struct OriginalDraw {
    pub(crate) source: u32,
    pub(crate) draw: gpu::Draw,
}
#[derive(Clone)]
pub(crate) struct OriginalFrame {
    pub(crate) row: usize,
    pub(crate) operation: String,
    pub(crate) properties: HigurashiProperties,
    /// Native width,height,current1498,previous14a0,allocated,retired,sampled39.
    pub(crate) screen: [u32; 7],
    pub(crate) events: Vec<ResourceEvent>,
    pub(crate) draw: Option<OriginalDraw>,
}
fn word(tokens: &mut std::str::SplitWhitespace<'_>) -> u32 {
    tokens.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(tokens: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(tokens))
}
fn skip(tokens: &mut std::str::SplitWhitespace<'_>, count: usize) {
    for _ in 0..count {
        assert!(tokens.next().is_some());
    }
}

/// No original executable is rerun here. Parent acceptance fingerprints retain
/// the actual101-profile/1374-action corpus; this parser exposes its unmodified
/// resources, snapshots and D42 words for new production integration tests.
pub(crate) fn original_frames() -> Vec<OriginalFrame> {
    let input = std::fs::read_to_string(
        std::env::var("SHIN_HIGURASHI_TEMPORAL_OWNERS_INPUT")
            .unwrap_or_else(|_| "/tmp/higurashi-temporal-owners-input.txt".into()),
    )
    .unwrap();
    let output = std::fs::read_to_string(
        std::env::var("SHIN_HIGURASHI_TEMPORAL_OWNERS_OUTPUT")
            .unwrap_or_else(|_| "/tmp/higurashi-temporal-owners-output.txt".into()),
    )
    .unwrap();
    assert_eq!(input.lines().count(), 1374);
    assert_eq!(output.lines().count(), 1375);
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    let mut draws = 0;
    let frames = input
        .lines()
        .chain(std::iter::once("E"))
        .zip(output.lines())
        .enumerate()
        .map(|(row, (operation, response))| {
            let mut request = operation.split_whitespace();
            match request.next().unwrap() {
                "N" => properties = HigurashiProperties::from_seeds([0; 4]),
                "T" => properties
                    .tweener_mut(39)
                    .unwrap()
                    .fast_forward_to(f32::from_bits(word(&mut request))),
                "P" => {
                    let property = word(&mut request) as usize;
                    properties
                        .tweener_mut(property)
                        .unwrap()
                        .fast_forward_to(f32::from_bits(word(&mut request)));
                }
                "Q" => {
                    let [target, time] = words::<2>(&mut request).map(f32::from_bits);
                    let flags = word(&mut request);
                    properties
                        .control(39, target, shin_core::time::Ticks::from_f32(time), flags)
                        .unwrap();
                }
                "A" => properties.advance(shin_core::time::Ticks::from_f32(f32::from_bits(word(
                    &mut request,
                )))),
                "Z" => properties.fast_forward(),
                _ => {}
            }
            let mut tokens = response.split_whitespace();
            assert_eq!(tokens.next(), Some("O"));
            assert_eq!(word(&mut tokens), row as u32);
            assert_eq!(tokens.next(), operation.split_whitespace().next());
            let mut screen = None;
            let mut events = vec![];
            let mut draw = None;
            while let Some(tag) = tokens.next() {
                match tag {
                    "N" => {
                        let [id, kind, width, height] = words(&mut tokens);
                        events.push(ResourceEvent::Allocate {
                            id,
                            kind,
                            extent: [width, height],
                        });
                    }
                    "I" => {
                        let [id, color] = words(&mut tokens);
                        events.push(ResourceEvent::InitialClear { id, color });
                    }
                    "X" => {
                        let [id, remaining] = words(&mut tokens);
                        events.push(ResourceEvent::Release { id, remaining });
                    }
                    "D" => {
                        let d = words::<42>(&mut tokens);
                        assert_eq!(&d[..5], &[6, 5, 1, 4, 0]);
                        assert!(draw.is_none());
                        draw = Some(OriginalDraw {
                            source: d[5],
                            draw: gpu::Draw {
                                program: d[0],
                                blend: d[1],
                                primitive: d[2],
                                count: d[3],
                                start: d[4],
                                depth: [7, 0],
                                stencil: [7, 0, 0, 0, 0xffff],
                                uniforms: vec![d[6..22].to_vec(), d[22..26].to_vec(), vec![0; 4]],
                                vertices: d[26..42].to_vec(),
                            },
                        });
                        draws += 1;
                    }
                    "S" => screen = Some(words(&mut tokens)),
                    "V" => {
                        skip(&mut tokens, 1);
                        let n = word(&mut tokens);
                        skip(&mut tokens, 6 * n as usize);
                    }
                    "B" => {
                        let n = word(&mut tokens);
                        skip(&mut tokens, 3 * n as usize);
                    }
                    "R" | "E" | "U" | "C" => skip(&mut tokens, 2),
                    "F" | "K" | "G" | "W" | "P" | "H" => skip(&mut tokens, 1),
                    unknown => panic!("Unreviewed temporal row{row} event {unknown}"),
                }
            }
            let screen = screen.unwrap_or([0; 7]); // Final cleanup has no snapshot.
            if let Some(draw) = &draw {
                assert_eq!(draw.source, screen[3]);
            }
            if screen[2] != 0 {
                assert_eq!(screen[6], properties.values()[39].to_bits());
            }
            OriginalFrame {
                row,
                operation: operation.into(),
                properties: properties.clone(),
                screen,
                events,
                draw,
            }
        })
        .collect();
    assert_eq!(draws, 128);
    frames
}

/// Draw exactly the original packet. Source association must be checked by the
/// caller's native resource-ID owner, separately from this GPU transport.
pub(crate) fn replay(
    renderer: &mut gpu::Renderer,
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    original: &OriginalDraw,
    source: TextureSource,
) {
    renderer.draw(context, target, &original.draw, Some(source), None);
}

fn controlled_source(extent: [u32; 2]) -> Vec<u8> {
    let [width, height] = extent;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            // Nonuniform RGB and independent alpha include zero-alpha/nonzero
            // RGB. This separates One from SrcAlpha and catches extra premul.
            pixels[i..i + 4].copy_from_slice(&[
                ((x * 191 / width + y * 37 / height + 43) % 256) as u8,
                ((x * 61 / width + y * 173 / height + 79) % 256) as u8,
                ((x * 149 / width + y * 83 / height + 17) % 256) as u8,
                [0, 67, 143, 255][((x / 83 + y / 57) % 4) as usize],
            ]);
        }
    }
    pixels
}
fn cases(frames: &[OriginalFrame]) -> Vec<(&'static str, &OriginalFrame)> {
    let mut selected = vec![];
    for extent in [[1280, 720], [1920, 1080]] {
        let choose = |predicate: &dyn Fn(&OriginalFrame) -> bool| {
            frames
                .iter()
                .find(|f| f.operation.trim() == "D" && f.screen[..2] == extent && predicate(f))
                .unwrap()
        };
        for weight in [1., 600., 752., 1000., 1400.] {
            selected.push((
                "scalar",
                choose(&|f| {
                    f.draw.is_some()
                        && f.properties.values()[39] == weight
                        && f.properties.values()[40..44] == [1000., 0., 0., 0.]
                }),
            ));
        }
        for angle in [100., 321.5, -401.25] {
            selected.push((
                "camera",
                choose(&|f| f.draw.is_some() && f.properties.values()[41] == angle),
            ));
        }
        selected.push((
            "previous-null",
            choose(&|f| f.draw.is_none() && f.properties.values()[39] == 600. && f.screen[3] == 0),
        ));
        selected.push((
            "fractional-positive",
            choose(&|f| f.draw.is_none() && f.properties.values()[39] == 0.5 && f.screen[3] != 0),
        ));
    }
    assert_eq!(selected.len(), 20);
    selected
}
fn differences(actual: &[u8], expected: &[u8]) -> (usize, u8) {
    assert_eq!(actual.len(), expected.len());
    (
        actual.iter().zip(expected).filter(|(a, b)| a != b).count(),
        actual
            .iter()
            .zip(expected)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap(),
    )
}

#[test]
#[ignore = "frozen original128 ba038 packets + literal primary6 SPIR-V; pure offscreen720p/1080p, no audio/window/user-save work"]
fn higurashi_afterimage_packets_pixels_match_original_glsl_and_blend5() {
    let frames = original_frames();
    let cases = cases(&frames);
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let mut tested = 0;
    let mut controls = 0;
    let mut summaries = vec![];
    for (name, frame) in cases {
        let extent = [frame.screen[0], frame.screen[1]];
        let pixels = controlled_source(extent);
        let source = GpuTexture::new_static_with_data(
            &device,
            &queue,
            Some("Controlled native afterimage transport source"),
            winit::dpi::PhysicalSize::new(extent[0], extent[1]),
            shin_render::TEXTURE_FORMAT,
            &pixels,
        );
        let packet = (frame.screen[3] != 0)
            .then(|| AfterimagePacket::new(&frame.properties))
            .flatten();
        assert_eq!(packet.is_some(), frame.draw.is_some());
        for output in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, output, |ctx, target| {
                initialize(ctx, target, 0);
                if let Some(packet) = &packet {
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Actual retained temporal afterimage packet",
                    );
                    packet.render(&mut pass, source.as_source());
                }
            });
            let expected = reference_render_to_texture(&device, &queue, output, |ctx, target| {
                initialize(ctx, target, 0);
                if let Some(draw) = &frame.draw {
                    replay(&mut renderer, ctx, target, draw, source.as_source());
                }
            });
            let (changed, maximum) = differences(&actual, &expected);
            assert_eq!(
                (changed, maximum),
                (0, 0),
                "native row{}/{name}/ctor{extent:?}/output{output:?}",
                frame.row
            );
            let colors = actual.chunks_exact(4).collect::<BTreeSet<_>>().len();
            if frame.draw.is_none() {
                assert_eq!(colors, 1);
            }
            if frame.draw.is_some() && frame.properties.values()[39] >= 600. {
                assert!(
                    colors > 3,
                    "native afterimage source must be spatially nonuniform"
                );
            }
            let wrong = reference_render_to_texture(&device, &queue, output, |ctx, target| {
                initialize(ctx, target, 0);
                if let Some(draw) = &frame.draw {
                    let mut wrong = draw.draw.clone();
                    wrong.blend = 2; // Actual ordinary SrcAlpha blend control.
                    renderer.draw(ctx, target, &wrong, Some(source.as_source()), None);
                }
            });
            controls += usize::from(wrong != expected);
            summaries.push(serde_json::json!({"native_row":frame.row,"case":name,"ctor_extent":extent,"output_extent":output,"source_id":frame.screen[3],"has_draw":frame.draw.is_some(),"different_channels":changed,"maximum_difference":maximum,"spatial_colors":colors,"ordinary_blend_differs":wrong!=expected}));
            tested += 1;
        }
    }
    assert_eq!(tested, 40);
    assert!(
        controls >= 20,
        "insufficient One versus SrcAlpha discrimination"
    );
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_AFTERIMAGE_PIXEL_CAPTURE") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("packet-summary.json"),
            serde_json::to_vec_pretty(&summaries).unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "Original afterimages: {tested} byte-exact full720p/1080p packet frames from frozen ba038/projection/weight/gates; {controls} native blend5 versus ordinary blend2 differential controls; nonuniform explicit texture transport, no Scene history completion claim/audio/window/user saves"
    );
}
