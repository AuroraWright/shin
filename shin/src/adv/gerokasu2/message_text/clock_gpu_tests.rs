//! Real controller updates feed the owned buffer's rendered fade values.
use super::*;
use crate::{
    adv::gerokasu2::{
        message::Gerokasu2Message,
        message_clock::{MessageEffects, MessageParams, VoiceRequest},
    },
    asset::font::gerokasu2::draw::gpu_tests::{Offscreen, compare, expected},
};
use shin_core::{
    format::{font::gerokasu2::Gerokasu2Font, rom::RomReader},
    primitives::{stateless_reader::StatelessFile, update::FrameId},
    time::Ticks,
    vm::command::types::MessageTextLayout,
};
use std::io::Read;

#[derive(Default)]
struct Effects {
    completions: Vec<u32>,
}
impl MessageEffects for Effects {
    fn voice_is_playing(&self) -> bool {
        false
    }
    fn stop_voice(&mut self) {}
    fn play_voice(&mut self, _: VoiceRequest) -> bool {
        false
    }
    fn observe_voice(&mut self, _: &str) {}
    fn completed(&mut self, id: u32) {
        self.completions.push(id);
    }
}

#[test]
#[ignore = "requires supplied ROM, GPU and exact Loopback Audio output"]
fn native_clock_updates_click_fast_forward_and_owned_glyph_fades_render_offscreen() {
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
        for mode in 0..6 {
            let text = Gerokasu2MessageText::layout_and_prepare(
                &gpu.device,
                &gpu.queue,
                [&font, &font],
                "@r@c900.A@v声_01.",
                mode,
                MessageTextLayout::Justify,
                50,
            )
            .unwrap();
            assert_eq!(text.character_count(), 1);
            let c = &text.characters[0];
            let color = c.color;
            let first = c.draw.vertices[0].position;
            let last = c.draw.vertices[3].position;
            let transform = shin_render::shin_orthographic_projection_matrix(
                first.x, last.x, last.y, first.y, -1.0, 1.0,
            );
            let samples = gpu.samples(
                c.glyph.as_texture_source().unwrap(),
                &c.draw.vertices,
                transform,
            );
            let outline_thresholds = c.draw.outline_thresholds;
            let fill_thresholds = c.draw.fill_thresholds;
            let mut message = Gerokasu2Message::new(
                text,
                MessageParams {
                    flags: 0,
                    mode,
                    message_id: 42,
                    message_arg: -1,
                    animate_open: false,
                },
            )
            .unwrap();
            let mut effects = Effects::default();
            let mut frame_id = FrameId::default();
            for frame in 0..8 {
                frame_id.advance();
                message
                    .update(frame_id, Ticks::from_f32(1.25), true, &mut effects)
                    .unwrap();
                if frame == 3 {
                    assert!(message.try_advance(false, &mut effects).unwrap());
                }
                if frame == 6 {
                    message.fast_forward(&mut effects).unwrap();
                }
                let progress = message.clock().progress().next().unwrap();
                assert_eq!(
                    message.text().characters[0].progress.to_bits(),
                    progress.to_bits()
                );
                // A duplicate frame must not progress either owner or render state.
                message
                    .update(frame_id, Ticks::from_f32(1.25), true, &mut effects)
                    .unwrap();
                assert_eq!(
                    message.clock().progress().next().unwrap().to_bits(),
                    progress.to_bits()
                );
                let pixels = gpu.draw(|pass| {
                    message
                        .render(
                            pass,
                            RenderRequestBuilder::new()
                                .color_blend_type(shin_render::ColorBlendType::Opaque),
                            transform,
                        )
                        .unwrap();
                });
                for (actual, distance) in pixels.into_iter().zip(samples.iter().copied()) {
                    let rgba =
                        if let Some([outline, fill]) = MessageGlyphDraw::colors(color, progress) {
                            let outline = outline.into_vec4().to_array();
                            let fill = fill.into_vec4().to_array();
                            let t = ((distance - fill_thresholds.x)
                                / (fill_thresholds.y - fill_thresholds.x))
                                .clamp(0.0, 1.0);
                            let coverage = (t * t) * (-2.0f32).mul_add(t, 3.0);
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
                        &format!("controller {path} mode{mode} frame{frame} fade{progress}"),
                    );
                }
                cases += 1;
            }
            assert!(message.clock().is_finished());
            assert_eq!(effects.completions, [42]);
        }
    }
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_MESSAGE_CLOCK_GPU fonts=2 modes=6 frames_per_mode=8 draw_cases={cases} pixels_per_case=64 channels_per_pixel=4 audio=Loopback Audio controller_tick_click_fast_forward_owned_fade_render=verified"
    );
}

fn verify_current_glyph(gpu: &mut Offscreen, message: &Gerokasu2Message) {
    let c = &message.text().characters[0];
    let first = c.draw.vertices[0].position;
    let last = c.draw.vertices[3].position;
    let transform = shin_render::shin_orthographic_projection_matrix(
        first.x, last.x, last.y, first.y, -1.0, 1.0,
    );
    let samples = gpu.samples(
        c.glyph.as_texture_source().unwrap(),
        &c.draw.vertices,
        transform,
    );
    let pixels = gpu.draw(|pass| {
        message
            .render(
                pass,
                RenderRequestBuilder::new().color_blend_type(shin_render::ColorBlendType::Opaque),
                transform,
            )
            .unwrap()
    });
    for (actual, distance) in pixels.into_iter().zip(samples) {
        let rgba = if let Some([outline, fill]) = MessageGlyphDraw::colors(c.color, c.progress) {
            let outline = outline.into_vec4().to_array();
            let fill = fill.into_vec4().to_array();
            let thresholds = c.draw.fill_thresholds;
            let t = ((distance - thresholds.x) / (thresholds.y - thresholds.x)).clamp(0.0, 1.0);
            if t * t * (-2.0f32).mul_add(t, 3.0) < 0.001 {
                expected(distance, c.draw.outline_thresholds, outline, outline, 0.0)
            } else {
                expected(distance, thresholds, fill, fill, 0.0)
            }
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        compare(
            actual,
            rgba,
            "replaced message cached glyph/owned vertices/fade",
        );
    }
}

#[test]
#[ignore = "requires supplied ROM, GPU and exact Loopback Audio output"]
fn replacing_prepared_messages_preserves_frame_and_window_state_and_renders_new_font_buffer() {
    let mut gpu = Offscreen::new();
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let fonts = ["/newrodin-medium.fnt", "/system.fnt"].map(|path| {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        Arc::new(GpuGerokasu2Font::new(Gerokasu2Font::new(bytes.into()).unwrap()).unwrap())
    });
    let mut cases = 0;
    for order in [[0, 1], [1, 0]] {
        for (before, after) in [(0, 3), (3, 0), (0, 1), (1, 4), (4, 5), (5, 0)] {
            let selected = [&fonts[order[0]], &fonts[order[1]]];
            let mut p = MessageParams {
                flags: 0,
                mode: before,
                message_id: 41,
                message_arg: -1,
                animate_open: false,
            };
            let old = Gerokasu2MessageText::layout_and_prepare(
                &gpu.device,
                &gpu.queue,
                selected,
                "@rA",
                before,
                MessageTextLayout::Justify,
                50,
            )
            .unwrap();
            let mut message = Gerokasu2Message::new(old, p).unwrap();
            let mut effects = Effects::default();
            let mut frame = FrameId::default();
            frame.advance();
            message
                .update(frame, Ticks::from_f32(1.25), true, &mut effects)
                .unwrap();
            verify_current_glyph(&mut gpu, &message);
            cases += 1;
            let previous_height = message.clock().height;
            message.request_autoplay();
            message.set_modal_hidden(true);
            // Both old and new buffers/handles remain owned during preparation.
            let new = Gerokasu2MessageText::layout_and_prepare(
                &gpu.device,
                &gpu.queue,
                selected,
                "@r@{B@}",
                after,
                MessageTextLayout::Justify,
                50,
            )
            .unwrap();
            p.mode = after;
            p.message_id = 42;
            message.replace(new, p).unwrap();
            assert!(message.clock().modal_hidden);
            assert_eq!(
                message.clock().first_line_width,
                message.text().lines()[0].width
            );
            assert_eq!(message.clock().text_size, message.text().layout_size());
            assert_eq!(
                message.clock().outgoing.len(),
                usize::from(!matches!((before, after), (0, 3) | (3, 0)))
            );
            if after == 5 {
                assert_eq!(message.clock().height, previous_height);
            }
            message.set_modal_hidden(false);
            message
                .update(frame, Ticks::from_f32(1.25), true, &mut effects)
                .unwrap();
            assert_eq!(
                message.clock().progress().next().unwrap(),
                0.0,
                "replacement retains layer frame tracker"
            );
            verify_current_glyph(&mut gpu, &message);
            cases += 1;
            frame.advance();
            message
                .update(frame, Ticks::from_f32(1.25), true, &mut effects)
                .unwrap();
            assert!(message.clock().progress().next().unwrap() > 0.0);
            verify_current_glyph(&mut gpu, &message);
            cases += 1;
            message.fast_forward(&mut effects).unwrap();
            assert_eq!(effects.completions, [42]);
            verify_current_glyph(&mut gpu, &message);
            cases += 1;
        }
    }
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_MESSAGE_INSTALL_GPU fonts=2 font_orders=2 replacement_pairs=6 draw_cases={cases} pixels_per_case=64 channels_per_pixel=4 audio=Loopback Audio prepared_replacement_retained_frame_window_new_font_buffer=verified"
    );
}
