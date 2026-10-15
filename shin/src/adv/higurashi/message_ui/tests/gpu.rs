use super::*;
use crate::adv::higurashi::{
    message_glyphs::tests::{
        self as glyph_tests,
        gpu::{gpu_assets, render_frame, replay_native},
    },
    message_owner::HigurashiMessageOwner,
    message_playback::MessageVoice,
};
use shin_core::{
    format::scenario::instruction_elements::MessageId,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};

fn background(pass: &mut RenderPass) {
    pass.clear(
        Some(shin_core::primitives::color::UnormColor::from_rgba(
            96, 136, 184, 255,
        )),
        Some(0),
        Some(1.0),
    );
}

fn replay(pass: &mut RenderPass, packets: &[Packet], textures: &HigurashiMessageTextures) {
    for p in packets {
        let stride = if p.program == 1 { 4 } else { 6 };
        let vertices = p
            .vertices
            .chunks_exact(stride)
            .map(|v| PosColTexVertex {
                position: vec3(
                    f32::from_bits(v[0]),
                    f32::from_bits(v[1]),
                    f32::from_bits(v[2]),
                ),
                color: shin_core::primitives::color::UnormColor(v[3]),
                texture_position: if stride == 6 {
                    vec2(f32::from_bits(v[4]), f32::from_bits(v[5]))
                } else {
                    Vec2::ZERO
                },
            })
            .collect();
        let texture = match p.texture {
            0 => None,
            1 => Some(UiTexture::Window),
            2 => Some(UiTexture::CursorRegular),
            3 => Some(UiTexture::CursorLast),
            4 => Some(UiTexture::KeyWait),
            _ => panic!("native texture"),
        };
        MessageUiDraw {
            texture,
            vertices,
            indices: p.indices.clone(),
            native_transform: Mat4::from_cols_array(&p.matrix.map(f32::from_bits)),
            stencil: if p.stencil == 0 {
                None
            } else {
                Some(p.stencil as u8)
            },
        }
        .render(pass, textures);
    }
}

/// Independent native UI/glyph packets with the actual outer traversal's
/// stencil reference. This reference uses desktop submission/shaders.
pub(in crate::adv::higurashi) fn replay_native_owner(
    pass: &mut RenderPass,
    owner: &HigurashiMessageOwner,
    text: &str,
    textures: &HigurashiMessageTextures,
    stencil: u8,
) {
    if owner.playback.input_blocked {
        return;
    }
    let glyphs = owner.glyphs().unwrap();
    let p = &owner.playback;
    let case = Case {
        slide: p.natural_slide,
        modal: p.modal_slide,
        height: p.height,
        name: glyphs.lines.first().map_or(0.0, |line| line.width),
        clock: p.ticks_since_wait,
        cursor: p.cursor,
        wait: match p.wait {
            None => -1,
            Some(WaitKind::Regular) => 0,
            Some(WaitKind::Last) => 1,
        },
        text_height: glyphs.size.y,
        opacity: owner.ui_settings.window_opacity,
        transparent: owner.ui_settings.transparent_window_setting,
        options: [0, 0],
        ..base(p.style)
    };
    let mut ui = native_layout(&[case], Some(text)).remove(0);
    for packet in &mut ui {
        if packet.stencil != 0 {
            packet.stencil = u32::from(stencil);
        }
    }
    replay(pass, &ui, textures);
    let glyph = glyph_tests::native(&glyph_tests::row(p.style as i32, 0, [0, 0], 1.0, text));
    glyph_tests::gpu::replay_native_at_stencil(pass, &glyph[0], glyphs, stencil + 2);
}
struct Voice;
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        false
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        false
    }
    fn stop(&mut self) {}
}
#[test]
#[ignore = "compares native UI packets and composed message owner offscreen at720p/1080p; no window/audio/saves"]
fn higurashi_message_windows_cursors_and_owner_match_native_packets() {
    let (device, queue, assets) = gpu_assets();
    let textures = shin_tasks::block_on(HigurashiMessageTextures::load(&assets)).unwrap();
    assert_eq!(textures.window.size_vec(), vec2(1648.0, 288.0));
    assert_eq!(textures.cursor_regular.size_vec(), vec2(120.0, 40.0));
    assert_eq!(textures.cursor_last.size_vec(), vec2(120.0, 40.0));
    assert_eq!(textures.key_wait.size_vec(), vec2(80.0, 40.0));
    let capture =
        std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_MESSAGE_UI_CAPTURES").unwrap());
    std::fs::create_dir_all(&capture).unwrap();
    let mut cases = Vec::new();
    for style in 0..5 {
        for variant in 0..6 {
            let mut c = base(style);
            c.clock = [0.0, 8.0, 16.0, 24.0, 45.5, 60.0][variant];
            c.wait = if variant % 2 == 0 { 0 } else { 1 };
            c.pose = variant == 2;
            c.outgoing = variant == 3;
            if variant == 4 {
                c.name = 0.0;
                c.slide = 0.5;
            }
            if variant == 5 {
                c.blocked = true;
            }
            cases.push(c);
        }
    }
    let packets = native(&cases);
    let mut frames = 0;
    for (i, (case, packets)) in cases.iter().zip(packets).enumerate() {
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| {
                background(pass);
                for draw in case.draws() {
                    draw.render(pass, &textures);
                }
            });
            let expected = render_frame(&device, &queue, size, |pass| {
                background(pass);
                replay(pass, &packets, &textures)
            });
            assert_eq!(actual, expected, "UI case {i} at {size:?}");
            if !packets.is_empty() {
                assert!(
                    actual
                        .chunks_exact(4)
                        .any(|pixel| pixel != [96, 136, 184, 255]),
                    "UI case {i} produced no visible pixels"
                );
            }
            std::fs::write(
                capture.join(format!("ui{i}-{}x{}.rgba", size.0, size.1)),
                actual,
            )
            .unwrap();
            frames += 1;
        }
    }
    for text in ["名前@r@bかな.@<漢字@>、続く。", ""] {
        let mut voice = Voice;
        for style in 0..5 {
            let mut owner = shin_tasks::block_on(HigurashiMessageOwner::load(&assets)).unwrap();
            owner
                .dispatch(
                    &Cmd::MSGINIT {
                        style,
                        updated_options: Some([-1, 2]),
                    },
                    &device,
                    &queue,
                    false,
                    0,
                    50,
                    0,
                    &voice,
                )
                .unwrap();
            owner
                .dispatch(
                    &Cmd::MSGSET {
                        msg_id: MessageId(123),
                        auto_wait: 0,
                        argument: -1,
                        text: text.into(),
                    },
                    &device,
                    &queue,
                    true,
                    0,
                    50,
                    0,
                    &voice,
                )
                .unwrap();
            owner.fast_forward(&mut voice);
            owner.playback.wait = Some(WaitKind::Last);
            owner.playback.ticks_since_wait = 24.0;
            owner.ui_settings = MessageUiSettings {
                window_opacity: 73,
                transparent_window_setting: 37,
            };
            let glyphs = owner.glyphs().unwrap();
            let name = glyphs.lines.first().map_or(0.0, |line| line.width);
            let c = Case {
                style: style as u32,
                height: owner.playback.height,
                name,
                wait: 1,
                clock: 24.0,
                cursor: owner.playback.cursor,
                text_height: glyphs.size.y,
                ..base(style as u32)
            };
            let ui = native_layout(&[c], Some(text));
            let glyph = glyph_tests::native(&glyph_tests::row(style, 0, [-1, 2], 1.0, text));
            for size in [(1280, 720), (1920, 1080)] {
                let actual = render_frame(&device, &queue, size, |pass| {
                    background(pass);
                    owner
                        .render(pass, &HigurashiTransform::default(), 7)
                        .unwrap()
                });
                let expected = render_frame(&device, &queue, size, |pass| {
                    background(pass);
                    replay(pass, &ui[0], &textures);
                    replay_native(pass, &glyph[0], owner.glyphs().unwrap());
                });
                assert_eq!(actual, expected, "message owner style {style} at {size:?}");
                std::fs::write(
                    capture.join(format!(
                        "owner{}-{style}-{}x{}.rgba",
                        text.is_empty(),
                        size.0,
                        size.1
                    )),
                    actual,
                )
                .unwrap();
                frames += 1;
            }
            // Animated close retains the speaker header while glyphs retire.
            owner
                .dispatch(
                    &Cmd::MSGCLOSE { wait: 0 },
                    &device,
                    &queue,
                    false,
                    0,
                    50,
                    0,
                    &voice,
                )
                .unwrap();
            let close = Case {
                style: style as u32,
                height: owner.playback.height,
                name,
                wait: -1,
                clock: 24.0,
                buffer: false,
                ..base(style as u32)
            };
            let ui = native(&[close]);
            for size in [(1280, 720), (1920, 1080)] {
                let actual = render_frame(&device, &queue, size, |pass| {
                    background(pass);
                    owner
                        .render(pass, &HigurashiTransform::default(), 7)
                        .unwrap()
                });
                let expected = render_frame(&device, &queue, size, |pass| {
                    background(pass);
                    replay(pass, &ui[0], &textures)
                });
                assert_eq!(actual, expected, "close owner style {style} at {size:?}");
                std::fs::write(
                    capture.join(format!(
                        "close{}-{style}-{}x{}.rgba",
                        text.is_empty(),
                        size.0,
                        size.1
                    )),
                    actual,
                )
                .unwrap();
                frames += 1;
            }
        }
    }
    println!("Higurashi message UI: {frames} whole RGBA frames match native packets at720p/1080p");
}
