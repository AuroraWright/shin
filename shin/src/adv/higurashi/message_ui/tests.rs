use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone)]
struct Case {
    style: u32,
    slide: f32,
    modal: f32,
    height: f32,
    name: f32,
    wait: i32,
    clock: f32,
    cursor: Vec2,
    options: [i32; 2],
    text_height: f32,
    opacity: u8,
    transparent: u8,
    buffer: bool,
    blocked: bool,
    outgoing: bool,
    pose: bool,
}
impl Case {
    fn row(&self) -> String {
        format!(
            "{} {:x} {:x} {:x} {:x} {} {:x} {:x} {:x} {} {} {:x} {} {} {} {} {} {}\n",
            self.style,
            self.slide.to_bits(),
            self.modal.to_bits(),
            self.height.to_bits(),
            self.name.to_bits(),
            self.wait,
            self.clock.to_bits(),
            self.cursor.x.to_bits(),
            self.cursor.y.to_bits(),
            self.options[0],
            self.options[1],
            self.text_height.to_bits(),
            self.opacity,
            self.transparent,
            u8::from(self.buffer),
            u8::from(self.blocked),
            u8::from(self.outgoing),
            u8::from(self.pose)
        )
    }
    fn state(&self) -> HigurashiMessagePlayback {
        let mut state = HigurashiMessagePlayback::default();
        state.style = self.style;
        state.natural_slide = self.slide;
        state.modal_slide = self.modal;
        state.height = self.height;
        state.wait = match self.wait {
            0 => Some(WaitKind::Regular),
            1 => Some(WaitKind::Last),
            _ => None,
        };
        state.ticks_since_wait = self.clock;
        state.cursor = self.cursor;
        state.options = self.options;
        state.input_blocked = self.blocked;
        if self.outgoing {
            state.outgoing = vec![
                OutgoingWindow {
                    style: 3,
                    slide: 0.5,
                    height: 1080.0,
                },
                OutgoingWindow {
                    style: 0,
                    slide: 0.25,
                    height: f32::from_bits(0x43a08000),
                },
                OutgoingWindow {
                    style: 1,
                    slide: 0.75,
                    height: 1080.0,
                },
            ];
        }
        state
    }

    fn settings(&self) -> MessageUiSettings {
        MessageUiSettings {
            window_opacity: self.opacity,
            transparent_window_setting: self.transparent,
        }
    }
    fn transform(&self) -> HigurashiTransform {
        if !self.pose {
            return Default::default();
        }
        let mut properties = crate::layer::higurashi_properties::INITIAL_VALUES.map(|v| v as f32);
        properties[0] = 17.25;
        properties[1] = -9.5;
        properties[12] = 875.0;
        properties[13] = 1125.0;
        let mut transform = HigurashiTransform::from_properties(&properties, [0.0; 3]);
        transform.compose_with(&HigurashiTransform::default(), 0);
        transform
    }
    fn draws(&self) -> Vec<MessageUiDraw> {
        message_ui_draws(
            &self.state(),
            self.name,
            vec2(1500.0, self.text_height),
            self.buffer,
            self.settings(),
            &self.transform(),
            7,
        )
    }
}
struct Packet {
    program: u32,
    texture: u32,
    stencil: u32,
    clip: u32,
    count: u32,
    matrix: [u32; 16],
    vertices: Vec<u32>,
    indices: Vec<u16>,
}
fn native(cases: &[Case]) -> Vec<Vec<Packet>> {
    native_layout(cases, None)
}
fn native_layout(cases: &[Case], text: Option<&str>) -> Vec<Vec<Packet>> {
    let mut command = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MESSAGE_UI").unwrap());
    command.args([
        std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
        std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
    ]);
    if let Some(text) = text {
        command.arg(text);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input: String = cases.iter().map(Case::row).collect();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("balanced ownership"));
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .zip(cases)
        .map(|(line, case)| {
            let mut words = line.split_whitespace();
            assert_eq!(words.next(), Some("R"));
            let num =
                |w: &mut std::str::SplitWhitespace<'_>| w.next().unwrap().parse::<u32>().unwrap();
            if text.is_some() {
                assert_eq!(words.next(), Some("L"));
                assert_eq!(num(&mut words), case.name.to_bits(), "Native speaker width");
                num(&mut words);
                assert_eq!(
                    num(&mut words),
                    case.text_height.to_bits(),
                    "Native text height"
                );
            }
            let mut draws = Vec::new();
            loop {
                match words.next().unwrap() {
                    "D" => {
                        let program = num(&mut words);
                        let texture = num(&mut words);
                        let stencil = num(&mut words);
                        let clip = num(&mut words);
                        let count = num(&mut words);
                        let n = num(&mut words);
                        let matrix = std::array::from_fn(|_| num(&mut words));
                        let vertices = (0..n).map(|_| num(&mut words)).collect();
                        let n = num(&mut words);
                        let indices = (0..n).map(|_| num(&mut words) as u16).collect();
                        draws.push(Packet {
                            program,
                            texture,
                            stencil,
                            clip,
                            count,
                            matrix,
                            vertices,
                            indices,
                        });
                    }
                    "E" => {
                        assert_eq!(num(&mut words), draws.len() as u32);
                        assert_eq!(num(&mut words), 0);
                        break;
                    }
                    tag => panic!("Unknown UI packet {tag}"),
                }
            }
            assert!(words.next().is_none());
            draws
        })
        .collect()
}
fn vertex_words(draw: &MessageUiDraw) -> Vec<u32> {
    draw.vertices
        .iter()
        .flat_map(|v| {
            let p = v.position;
            let c = v.color;
            let uv = v.texture_position;
            let mut out = vec![p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), c.0];
            if draw.texture.is_some() {
                out.extend(uv.to_array().map(f32::to_bits));
            }
            out
        })
        .collect()
}
fn texture_id(texture: Option<UiTexture>) -> u32 {
    match texture {
        None => 0,
        Some(UiTexture::Window) => 1,
        Some(UiTexture::CursorRegular) => 2,
        Some(UiTexture::CursorLast) => 3,
        Some(UiTexture::KeyWait) => 4,
    }
}
fn base(style: u32) -> Case {
    Case {
        style,
        slide: 1.0,
        modal: 1.0,
        height: 357.0,
        name: 134.75,
        wait: 0,
        clock: 0.0,
        cursor: vec2(500.0, 180.5),
        options: [-1, 2],
        text_height: 314.25,
        opacity: 73,
        transparent: 37,
        buffer: true,
        blocked: false,
        outgoing: false,
        pose: false,
    }
}
#[test]
#[ignore = "executes actual updated window/cursor drawing with controlled message metadata"]
fn higurashi_native_message_windows_and_cursors() {
    let mut cases = Vec::new();
    for style in 0..5 {
        for wait in [-1, 0, 1] {
            for clock in [0.0, 7.999, 8.0, 16.0, 24.0, 32.0, 45.5, 60.0, 121.75] {
                for name in [0.0, 134.75, 1500.0] {
                    for pose in [false, true] {
                        let mut case = base(style);
                        case.wait = wait;
                        case.clock = clock;
                        case.name = name;
                        case.pose = pose;
                        case.outgoing = clock == 24.0;
                        cases.push(case);
                    }
                }
            }
        }
        for (slide, modal) in [
            (0.0, 1.0),
            (0.125, 1.0),
            (1.0, 0.25),
            (1.0, 0.0),
            (0.75, 0.5),
        ] {
            for height in [100.0, 357.0, 581.25, 1080.0] {
                for (opacity, transparent) in [(0, 0), (100, 100), (255, 255)] {
                    let mut case = base(style);
                    case.slide = slide;
                    case.modal = modal;
                    case.height = height;
                    case.opacity = opacity;
                    case.transparent = transparent;
                    case.outgoing = true;
                    cases.push(case);
                }
            }
        }
        for (buffer, blocked) in [(false, false), (true, true)] {
            let mut case = base(style);
            case.buffer = buffer;
            case.blocked = blocked;
            case.outgoing = true;
            cases.push(case);
        }
        for options in [[i32::MAX, i32::MIN], [2, -3]] {
            let mut case = base(style);
            case.options = options;
            cases.push(case);
        }
    }
    let packets = native(&cases);
    assert_eq!(packets.len(), cases.len());
    let mut draws = 0;
    for (i, (case, packets)) in cases.iter().zip(packets).enumerate() {
        let rust = case.draws();
        assert_eq!(rust.len(), packets.len(), "case {i} draw count");
        for (j, (rust, native)) in rust.iter().zip(packets).enumerate() {
            assert_eq!(
                texture_id(rust.texture),
                native.texture,
                "case {i} draw {j} texture"
            );
            assert_eq!(if rust.texture.is_some() { 2 } else { 1 }, native.program);
            assert_eq!(u32::from(rust.stencil.unwrap_or(0)), native.stencil);
            assert_eq!(native.clip, 0);
            assert_eq!(
                if rust.indices.is_empty() {
                    rust.vertices.len()
                } else {
                    rust.indices.len()
                } as u32,
                native.count
            );
            assert_eq!(rust.indices, native.indices);
            assert_eq!(
                vertex_words(rust),
                native.vertices,
                "case {i} draw {j} vertices"
            );
            assert_eq!(
                rust.native_transform.to_cols_array().map(f32::to_bits),
                native.matrix,
                "case {i} draw {j} matrix"
            );
            draws += 1;
        }
    }
    println!(
        "Higurashi message UI: {} native cases/{} draw packets match bitwise",
        cases.len(),
        draws
    );
}

pub(in crate::adv::higurashi) mod gpu;
