//! Runtime shared-layout ownership and actual cached message SDF passes.
use super::*;
use crate::asset::font::gerokasu2::draw::gpu_tests::{Offscreen, compare, expected};
use shin_core::{
    format::{font::gerokasu2::Gerokasu2Font, rom::RomReader},
    layout::{LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults},
    primitives::stateless_reader::StatelessFile,
};
use std::io::Read;

#[test]
#[ignore = "requires supplied ROM, GPU and exact Loopback Audio output"]
fn shared_layout_cached_handles_owned_quads_and_message_fade_render_offscreen() {
    let mut gpu = Offscreen::new();
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut cases = 0;
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font =
            Arc::new(GpuGerokasu2Font::new(Gerokasu2Font::new(bytes.into()).unwrap()).unwrap());
        for source in ["A@vtestvoice.", "@c900.A@vtestvoice.", "@c0.A@vtestvoice."] {
            // Caller parameters are explicit; this does not assign native
            // MessageLayer style/font-selection or parser/reflow semantics.
            let (commands, lines, layout_size) = MessageTextLayouter::new(
                font.metrics(),
                font.metrics(),
                LayoutParams {
                    text_size: 50.0,
                    base_font_horizontal_scale: 0.9697,
                    ..Default::default()
                },
                MessageTextLayouterDefaults {
                    color: 999,
                    draw_speed: 50,
                    fade: 200,
                },
            )
            .parse(source);
            let mut text = Gerokasu2MessageText::prepare(
                &gpu.device,
                &gpu.queue,
                [&font, &font],
                commands,
                lines,
                layout_size,
            )
            .unwrap();
            assert_eq!(text.character_count(), 1);
            assert!(matches!(
                text.commands()[text.character_command_index(0).unwrap()],
                Command::Char(_)
            ));
            assert!(
                text.commands()
                    .iter()
                    .any(|c| matches!(c, Command::Voice(_)))
            );
            assert!(text.set_progress(1, 1.0).is_err());
            assert!(!text.lines().is_empty());
            let c = &text.characters[0];
            let color = c.color;
            let draw = &c.draw;
            let first = draw.vertices[0].position;
            let last = draw.vertices[3].position;
            let transform = shin_render::shin_orthographic_projection_matrix(
                first.x, last.x, last.y, first.y, -1.0, 1.0,
            );
            let samples = gpu.samples(
                c.glyph.as_texture_source().unwrap(),
                &draw.vertices,
                transform,
            );
            let outline_thresholds = draw.outline_thresholds;
            let fill_thresholds = draw.fill_thresholds;
            for progress in [0.0, 0.4, 1.0, f32::NAN] {
                text.set_progress(0, progress).unwrap();
                let pixels = gpu.draw(|pass| {
                    text.render(
                        pass,
                        RenderRequestBuilder::new()
                            .color_blend_type(shin_render::ColorBlendType::Opaque),
                        transform,
                    )
                    .unwrap()
                });
                for (actual, distance) in pixels.into_iter().zip(samples.iter().copied()) {
                    let rgba =
                        if let Some([outline, fill]) = MessageGlyphDraw::colors(color, progress) {
                            let outline = outline.into_vec4().to_array();
                            let fill = fill.into_vec4().to_array();
                            let fill_t = ((distance - fill_thresholds.x)
                                / (fill_thresholds.y - fill_thresholds.x))
                                .clamp(0.0, 1.0);
                            let coverage = (fill_t * fill_t) * (-2.0f32).mul_add(fill_t, 3.0);
                            if coverage < 0.001 {
                                expected(distance, outline_thresholds, outline, outline, 0.0)
                            } else {
                                expected(distance, fill_thresholds, fill, fill, 0.0)
                            }
                        } else {
                            [0.0, 0.0, 0.0, 1.0]
                        };
                    compare(
                        actual,
                        rgba,
                        &format!("message {path} source{source:?} progress{progress}"),
                    );
                }
                cases += 1;
            }
        }
    }
    println!(
        "GEROKASU2_MESSAGE_TEXT_GPU fonts=2 shared_layout_messages=6 draw_cases={cases} pixels_per_case=64 channels_per_pixel=4 audio=Loopback Audio owned_buffer_fade_outline_fill=verified"
    );
}

#[test]
#[ignore = "requires supplied ROM, GPU and exact Loopback Audio output"]
fn authored_layout_profiles_prepare_every_message_with_cached_native_geometry() {
    use shin_core::format::scenario::{
        gerokasu2::{Gerokasu2Scenario, bytecode::Instruction},
        instruction_elements::CodeAddress,
    };
    let gpu = Offscreen::new();
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |path| {
        let mut data = Vec::new();
        let file = rom.find_file(path).unwrap();
        rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
        data
    };
    let normal = Arc::new(
        GpuGerokasu2Font::new(Gerokasu2Font::new(read("/newrodin-medium.fnt").into()).unwrap())
            .unwrap(),
    );
    let bold = Arc::new(
        GpuGerokasu2Font::new(Gerokasu2Font::new(read("/system.fnt").into()).unwrap()).unwrap(),
    );
    let scenario = Gerokasu2Scenario::new(read("/main.snr").into()).unwrap();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut messages = 0;
    let mut characters = 0;
    let mut collapsed_spaces = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let tail = &scenario.raw()[reader.position().0 as usize..];
        if tail.len() < 16 && tail.iter().all(|&b| b == 0) {
            break;
        }
        if let Instruction::MessageSet { text, .. } = reader.read().unwrap() {
            let prepared = Gerokasu2MessageText::layout_and_prepare(
                &gpu.device,
                &gpu.queue,
                [&normal, &bold],
                text.as_str(),
                0,
                shin_core::vm::command::types::MessageTextLayout::Justify,
                50,
            )
            .unwrap();
            for c in prepared.commands() {
                if let Command::Char(c) = c {
                    characters += 1;
                    if c.horizontal_scale == 0.0 {
                        assert_eq!(c.codepoint, '　');
                        collapsed_spaces += 1;
                    }
                }
            }
            assert_eq!(
                prepared.character_count(),
                prepared
                    .commands()
                    .iter()
                    .filter(|c| matches!(c, Command::Char(_)))
                    .count()
            );
            messages += 1;
        }
    }
    assert_eq!(messages, 5926);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_MESSAGE_LAYOUT_UPLOAD messages={messages} characters={characters} collapsed_spaces={collapsed_spaces} profiles=mode0_justify_speed50 original_layout_to_FNT6_cache_owned_buffer=verified audio=Loopback Audio"
    );
}
