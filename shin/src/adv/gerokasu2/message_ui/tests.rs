use super::*;
use crate::adv::gerokasu2::message_clock::MessageParams;
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    cases: usize,
    draws: usize,
    words: usize,
    text: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_MESSAGE_UI_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_NORMAL_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_BOLD_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            cases: 0,
            draws: 0,
            words: 0,
            text: 0,
        }
    }
    fn words(&mut self, expected: &[u32], scope: &str) {
        let mut bytes = vec![0; expected.len() * 4];
        self.output.read_exact(&mut bytes).unwrap();
        let actual = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        let diffs = actual
            .iter()
            .zip(expected)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(8)
            .collect::<Vec<_>>();
        assert!(
            diffs.is_empty(),
            "UI case{} {scope} differences{diffs:?}",
            self.cases
        );
        self.words += expected.len();
    }
    fn send(
        &mut self,
        clock: &MessageClock,
        ready: bool,
        settings: MessageUiSettings,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        pass: PassKind,
    ) {
        let mut row = vec![
            clock.params.mode as u32,
            clock.slide.to_bits(),
            clock.modal_slide.to_bits(),
            clock.height.to_bits(),
            clock.background.to_bits(),
            clock.wait_kind as u32,
            clock.ticks_since_wait.to_bits(),
            clock.cursor.x.to_bits(),
            clock.cursor.y.to_bits(),
            clock.text_size.y.to_bits(),
            clock.first_line_width.to_bits(),
            u32::from(settings.window_opacity),
            u32::from(ready),
            clock.outgoing.len() as u32,
        ];
        row.extend(local.to_cols_array().map(f32::to_bits));
        row.extend(parent.to_cols_array().map(f32::to_bits));
        for w in &clock.outgoing {
            row.extend([w.mode as u32, w.slide.to_bits(), w.height.to_bits()]);
        }
        row.extend([u32::from(stencil), u32::from(pass == PassKind::Opaque)]);
        let mut input = Vec::new();
        for word in row {
            input.extend(word.to_le_bytes());
        }
        self.input.as_mut().unwrap().write_all(&input).unwrap();
    }
    fn compare(
        &mut self,
        clock: &MessageClock,
        ready: bool,
        settings: MessageUiSettings,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        pass: PassKind,
    ) {
        self.send(clock, ready, settings, parent, local, stencil, pass);
        let expected = plan(clock, ready, settings, parent, local, stencil, pass).unwrap();
        let mut magic = [0; 4];
        self.output.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"GMU1");
        self.words(&[expected.draws.len() as u32], "draw count");
        for draw in &expected.draws {
            let texture = match draw.texture {
                None => 0,
                Some(UiTexture::Window) => 1,
                Some(UiTexture::KeyWait) => 2,
            };
            let program = if texture == 0 { 1 } else { 2 };
            let stride = if texture == 0 { 4 } else { 6 };
            let mut words = vec![program, 2, texture, u32::from(draw.stencil), 0, stride * 4];
            words.extend(draw.transform.to_cols_array().map(f32::to_bits));
            for vertex in &draw.vertices {
                let position = vertex.position;
                words.extend(position.to_array().map(f32::to_bits));
                words.push(vertex.color.0);
                if texture != 0 {
                    let texture_position = vertex.texture_position;
                    words.extend(texture_position.to_array().map(f32::to_bits));
                }
            }
            words.resize(46, 0);
            self.words(&words, "window/cursor/fill matrix/vertices/color/stencil");
        }
        self.words(
            &[u32::from(expected.text_transform.is_some())],
            "text submission gate",
        );
        self.words(
            &expected
                .text_transform
                .map_or([0; 16], |m| m.to_cols_array().map(f32::to_bits)),
            "text transform",
        );
        self.cases += 1;
        self.draws += expected.draws.len();
        self.text += usize::from(expected.text_transform.is_some());
    }
    fn read_words(&mut self, count: usize) -> Vec<u32> {
        let mut bytes = vec![0; count * 4];
        self.output.read_exact(&mut bytes).unwrap();
        bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }
    fn capture(
        &mut self,
        clock: &MessageClock,
        ready: bool,
        settings: MessageUiSettings,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        pass: PassKind,
    ) -> MessageUiPlan {
        self.send(clock, ready, settings, parent, local, stencil, pass);
        let mut magic = [0; 4];
        self.output.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"GMU1");
        let count = self.read_words(1)[0] as usize;
        let mut draws = Vec::new();
        for _ in 0..count {
            let row = self.read_words(46);
            assert_eq!(row[1], 2);
            assert_eq!(row[4], 0);
            let texture = match row[2] {
                0 => None,
                1 => Some(UiTexture::Window),
                2 => Some(UiTexture::KeyWait),
                _ => panic!("native texture"),
            };
            let stride = if row[0] == 1 { 4 } else { 6 };
            assert_eq!(row[5], 4 * stride);
            let vertices = std::array::from_fn(|i| {
                let at = 22 + i * stride as usize;
                PosColTexVertex {
                    position: vec3(
                        f32::from_bits(row[at]),
                        f32::from_bits(row[at + 1]),
                        f32::from_bits(row[at + 2]),
                    ),
                    color: UnormColor(row[at + 3]),
                    texture_position: if stride == 6 {
                        vec2(f32::from_bits(row[at + 4]), f32::from_bits(row[at + 5]))
                    } else {
                        Vec2::ZERO
                    },
                }
            });
            draws.push(UiDraw {
                texture,
                vertices,
                stencil: row[3] as u8,
                transform: Mat4::from_cols_array(&std::array::from_fn(|i| {
                    f32::from_bits(row[6 + i])
                })),
            });
        }
        let has_text = self.read_words(1)[0] != 0;
        let matrix = self.read_words(16);
        self.cases += 1;
        self.draws += count;
        self.text += usize::from(has_text);
        self.words += 18 + count * 46;
        MessageUiPlan {
            draws,
            text_transform: has_text.then(|| {
                Mat4::from_cols_array(&std::array::from_fn(|i| f32::from_bits(matrix[i])))
            }),
            text_stencil: stencil.wrapping_add(2),
        }
    }
    fn finish(mut self) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_MESSAGE_UI cases={} draws={} text_transforms={} words={} original_full_renderer_window_cursor_quad_matrix_color_stencil=verified",
            self.cases, self.draws, self.text, self.words
        );
    }
}

#[cfg(test)]
pub(super) mod gpu_tests;
fn clock() -> MessageClock {
    MessageClock::new(
        &[],
        &[],
        Vec2::ZERO,
        MessageParams {
            flags: 0,
            mode: 0,
            message_id: 0,
            message_arg: 0,
            animate_open: false,
        },
    )
    .unwrap()
}

#[test]
fn later_window_modes_hide_mode2_text_and_reject_unassigned_visible_modes() {
    let mut c = clock();
    let settings = MessageUiSettings::default();
    c.params.mode = 2;
    let p = plan(
        &c,
        true,
        settings,
        Mat4::IDENTITY,
        Mat4::IDENTITY,
        7,
        PassKind::Transparent,
    )
    .unwrap();
    assert!(p.draws.is_empty() && p.text_transform.is_none());
    c.params.mode = 5;
    assert!(
        plan(
            &c,
            true,
            settings,
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            7,
            PassKind::Transparent
        )
        .is_err()
    );
    assert!(
        plan(
            &c,
            false,
            settings,
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            7,
            PassKind::Transparent
        )
        .unwrap()
        .draws
        .is_empty()
    );
}

#[test]
#[ignore = "requires supplied executable/fonts and guarded original message UI oracle"]
fn native_whole_message_renderer_window_cursor_transform_color_and_pass_matrix() {
    let mut native = Native::new();
    let mut c = clock();
    let mut index = 0;
    for (mode, slide, modal, wait, ready, outgoing, pass) in itertools::iproduct!(
        0..5,
        [0.0, 0.1, 0.8, 1.0],
        [0.0, 0.25, 1.0],
        [-1, 0, 1],
        [false, true],
        [false, true],
        [PassKind::Opaque, PassKind::Transparent]
    ) {
        c.params.mode = mode;
        c.slide = slide;
        c.modal_slide = modal;
        c.wait_kind = wait;
        c.height = [259.0, 350.25, 1080.0][index % 3];
        c.text_size.y = [140.0, 1100.0, 1030.5][index % 3];
        c.first_line_width = 360.0;
        c.background = [0.0, 0.25, 1.0][index % 3];
        c.ticks_since_wait = [0.0, 7.25, 31.0, 91.0, -10.5, 10000.0][index % 6];
        c.cursor = vec2(125.25, 80.5);
        c.outgoing = if outgoing {
            vec![
                OutgoingWindow {
                    mode: 0,
                    slide: 0.75,
                    height: 259.0,
                },
                OutgoingWindow {
                    mode: 3,
                    slide: 0.25,
                    height: 300.25,
                },
                OutgoingWindow {
                    mode: 1,
                    slide: 0.5,
                    height: 1080.0,
                },
            ]
        } else {
            Vec::new()
        };
        let parent = if index % 3 == 0 {
            Mat4::IDENTITY
        } else if index % 3 == 1 {
            shin_render::shin_orthographic_projection_matrix(
                -960.0, 960.0, 540.0, -540.0, -1.0, 1.0,
            )
        } else {
            Mat4::from_cols_array(&[
                0.25, 0.125, 0.0, 0.0, -0.375, 1.125, 0.0, 0.0, 0.0, 0.0, -0.5, 0.0, 13.25, -7.5,
                0.5, 1.0,
            ])
        };
        let local = if index % 2 == 0 {
            Mat4::IDENTITY
        } else {
            Mat4::from_translation(vec3(-960.0, -540.0, 0.0))
        };
        native.compare(
            &c,
            ready,
            MessageUiSettings {
                window_opacity: [0, 50, 100, 255][index % 4],
            },
            parent,
            local,
            [0, 7, 253, 255][index % 4],
            pass,
        );
        index += 1;
    }
    // Unknown outgoing modes emit nothing; current modes are only assigned
    // for full rendering when a text buffer is ready.
    for mode in [-1, 5, 31] {
        c.params.mode = mode;
        c.outgoing.clear();
        c.outgoing.push(OutgoingWindow {
            mode,
            slide: 0.75,
            height: 259.0,
        });
        native.compare(
            &c,
            false,
            MessageUiSettings::default(),
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            7,
            PassKind::Transparent,
        );
    }
    native.finish();
}
