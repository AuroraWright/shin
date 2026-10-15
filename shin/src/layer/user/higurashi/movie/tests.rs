use super::*;
mod gpu;
use std::collections::VecDeque;

const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-movie-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-movie-capture/output.txt"
));

struct Profile {
    gain: Gain,
    snapshots: VecDeque<Volume>,
    renders: VecDeque<[i32; 65]>,
    waits: VecDeque<(i32, i32, bool)>,
}
fn profiles() -> Vec<Profile> {
    let mut profiles: Vec<Profile> = Vec::new();
    let mut percentages = [0; 3];
    for line in INPUT.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "N" => {
                percentages = std::array::from_fn(|i| fields[4 + i].parse().unwrap());
                profiles.push(Profile {
                    gain: Gain::unpack(fields[3].parse().unwrap()),
                    snapshots: VecDeque::new(),
                    renders: VecDeque::new(),
                    waits: VecDeque::new(),
                });
            }
            "V" => percentages = std::array::from_fn(|i| fields[1 + i].parse().unwrap()),
            "R" => profiles
                .last_mut()
                .unwrap()
                .renders
                .push_back(std::array::from_fn(|i| fields[3 + i].parse().unwrap())),
            "W" => profiles.last_mut().unwrap().waits.push_back((
                fields[1].parse().unwrap(),
                fields[2].parse().unwrap(),
                fields[4] == "1",
            )),
            _ => {}
        }
        // These original opcodes each emit exactly one actual Player snapshot.
        if matches!(
            fields[0],
            "N" | "F" | "U" | "Q" | "D" | "P" | "R" | "A" | "C" | "S" | "V" | "K" | "B" | "T"
        ) {
            let profile = profiles.last_mut().unwrap();
            profile
                .snapshots
                .push_back(profile.gain.sample(percentages));
        }
    }
    profiles
}
struct Native<'a> {
    words: Vec<&'a str>,
    index: usize,
}
impl<'a> Native<'a> {
    fn new(line: &'a str) -> Self {
        Self {
            words: line.split_whitespace().collect(),
            index: 4,
        }
    }
    fn event(&mut self) -> Option<(&'a str, Vec<u32>)> {
        let tag = *self.words.get(self.index)?;
        self.index += 1;
        let count = match tag {
            "O" => {
                // Full base owner snapshot includes mangled class name.
                self.index += 338;
                assert_eq!(&self.words[self.index..self.index + 3], &["D", "7", "3"]);
                self.index += 3;
                3
            }
            "w" => 4 + 3 * self.words[self.index + 3].parse::<usize>().unwrap(),
            "p" => {
                self.index += 1;
                return Some((tag, Vec::new()));
            }
            "h" => {
                // NV12 transport byte hashes are u64, not shader uniform words.
                self.index += 3;
                return Some((tag, Vec::new()));
            }
            "P" => 26,
            "D" => 113,
            "E" => 9,
            "M" | "J" | "K" | "F" | "Q" | "l" | "r" => 2,
            "W" | "A" | "B" | "y" => 3,
            "b" | "v" | "a" | "q" | "f" | "j" => 1,
            "N" | "n" | "T" | "Z" => 4,
            "S" => 5,
            _ => panic!("Unexpected original Movie event{tag}"),
        };
        let result = self.words[self.index..self.index + count]
            .iter()
            .map(|word| {
                let signed: i64 = word.parse().unwrap();
                assert!((i32::MIN as i64..=u32::MAX as i64).contains(&signed));
                signed as u32
            })
            .collect();
        self.index += count;
        Some((tag, result))
    }
}
fn vertex_words(vertices: &[PosTexVertex; 4]) -> Vec<u32> {
    vertices
        .iter()
        .flat_map(|vertex| {
            // PosTexVertex is packed; copy fields before taking Vec2 references.
            let position = vertex.position;
            let texture_position = vertex.texture_position;
            position
                .to_array()
                .into_iter()
                .chain(texture_position.to_array())
                .map(f32::to_bits)
        })
        .collect()
}
fn program(operation: LayerFragmentShader) -> u32 {
    match operation {
        LayerFragmentShader::Default => 6,
        LayerFragmentShader::Mono => 9,
        LayerFragmentShader::Fill => 7,
        LayerFragmentShader::Fill2 => 8,
        LayerFragmentShader::Negative => 10,
        LayerFragmentShader::Gamma => 11,
    }
}
fn blend(value: ColorBlendType) -> u32 {
    match value {
        ColorBlendType::Layer1 => 2,
        ColorBlendType::Layer2 => 3,
        ColorBlendType::HigurashiLayer3 => 4,
        _ => panic!("Movie final draw must use ordinary blend2/3/4"),
    }
}

#[test]
fn higurashi_movie_gain_matches_901_original_states_and_103_packed_owner_descriptors() {
    let mut profiles = profiles();
    assert_eq!(profiles.len(), 111);
    let mut snapshots = 0;
    let mut owners = 0;
    let mut audio_types = 0;
    for (profile_index, line) in OUTPUT.lines().enumerate() {
        let profile = &mut profiles[profile_index];
        let mut native = Native::new(line);
        while let Some((tag, words)) = native.event() {
            match tag {
                "O" => {
                    assert_eq!(profile.gain.category, words[0]);
                    assert_eq!(profile.gain.local.to_bits(), words[1]);
                    assert_eq!(words[2], 0x931ac);
                    owners += 1;
                }
                "P" => {
                    let gain = profile.snapshots.pop_front().unwrap();
                    assert_eq!(
                        gain.0.to_bits(),
                        words[11],
                        "Movie gain profile{}",
                        profile_index + 1
                    );
                    snapshots += 1;
                }
                "A" => {
                    assert_eq!(words[2], u32::from(profile.gain.category_one_source()));
                    audio_types += 1;
                }
                _ => {}
            }
        }
        assert!(profile.snapshots.is_empty());
    }
    assert_eq!(snapshots, 901);
    assert_eq!(owners, 103);
    assert!(audio_types > 0);
}

#[test]
fn higurashi_movie_original_82_conversion_and_76_ordinary_packets() {
    let mut profiles = profiles();
    let mut conversions = 0;
    let mut final_packets = 0;
    let mut render_visits = 0;
    for (profile_index, line) in OUTPUT.lines().enumerate() {
        let profile = &mut profiles[profile_index];
        let mut native = Native::new(line);
        let mut frame = None;
        let mut expected = None;
        while let Some((tag, words)) = native.event() {
            match tag {
                "P" => {
                    assert!(
                        expected.is_none(),
                        "Missing native Movie final packet profile{}",
                        profile_index + 1
                    );
                    frame = (words[9] != 0).then(|| FrameInfo {
                        size: vec2(f32::from_bits(words[13]), f32::from_bits(words[14])),
                        origin: vec2(f32::from_bits(words[17]), f32::from_bits(words[18])),
                        extent: vec2(f32::from_bits(words[19]), f32::from_bits(words[20])),
                    });
                }
                "Q" => {
                    let values = profile.renders.pop_front().unwrap();
                    let mut properties = HigurashiProperties::from_seeds([0; 4]);
                    for (index, value) in values.into_iter().enumerate() {
                        properties
                            .tweener_mut(index)
                            .unwrap()
                            .fast_forward_to(value as f32);
                    }
                    let kind = if words[0] == 1 {
                        PassKind::Opaque
                    } else {
                        PassKind::Transparent
                    };
                    expected = frame.and_then(|frame| {
                        Packet::new(frame, &properties, &HigurashiTransform::default(), kind)
                            .unwrap()
                    });
                    render_visits += 1;
                }
                "D" if words[3] == 20 => {
                    let packet = ConversionPacket::new((words[7], words[8]));
                    assert_eq!(words[4], 1);
                    assert_eq!(&words[21..24], &[64, 16, 48]);
                    assert_eq!(
                        packet.matrix.to_cols_array().map(f32::to_bits),
                        words[25..41],
                        "Original conversion matrix profile{}",
                        profile_index + 1
                    );
                    assert_eq!(packet.bias.to_array().map(f32::to_bits), words[41..45]);
                    let rows: Vec<_> = packet
                        .rows
                        .into_iter()
                        .flat_map(|row| row.to_array().map(f32::to_bits))
                        .collect();
                    assert_eq!(rows, words[57..69]);
                    assert_eq!(vertex_words(&packet.vertices), words[89..105]);
                    conversions += 1;
                }
                "D" => {
                    let packet = expected.take().expect("Original emitted final packet");
                    assert_eq!(program(packet.operation), words[3]);
                    assert_eq!(blend(packet.blend), words[4]);
                    assert_eq!(&words[21..24], &[64, 16, 16]);
                    assert_eq!(
                        packet.matrix.to_cols_array().map(f32::to_bits),
                        words[25..41],
                        "Original final Movie matrix profile{}",
                        profile_index + 1
                    );
                    assert_eq!(packet.color.into_array().map(f32::to_bits), words[41..45]);
                    assert_eq!(packet.param.to_array().map(f32::to_bits), words[57..61]);
                    assert_eq!(vertex_words(&packet.vertices), words[89..105]);
                    final_packets += 1;
                }
                _ => {}
            }
        }
        assert!(profile.renders.is_empty());
    }
    assert_eq!(conversions, 82);
    assert_eq!(final_packets, 76);
    assert_eq!(render_visits, 152);
}

#[test]
fn higurashi_movie_retains_authored_clip_and_opaque_regular_blend() {
    let mut properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
    let frame = FrameInfo::decoded((1280, 720));
    let opaque = Packet::new(
        frame,
        &properties,
        &HigurashiTransform::default(),
        PassKind::Opaque,
    )
    .unwrap()
    .unwrap();
    assert_eq!(opaque.blend, ColorBlendType::Layer1);
    assert!(
        Packet::new(
            frame,
            &properties,
            &HigurashiTransform::default(),
            PassKind::Transparent
        )
        .unwrap()
        .is_none()
    );
    properties.tweener_mut(44).unwrap().fast_forward_to(1.);
    assert!(
        Packet::new(
            frame,
            &properties,
            &HigurashiTransform::default(),
            PassKind::Opaque
        )
        .unwrap()
        .is_some()
    );
    assert!(super::super::leaf_clip::Clip::from_properties(&properties).active());
}

#[test]
fn higurashi_movie_wait_modes_match_645_original_polls_with_captured_missing_targets() {
    let mut profiles = profiles();
    let mut comparisons = 0;
    for (profile_index, line) in OUTPUT.lines().enumerate() {
        let profile = &mut profiles[profile_index];
        let mut status = 0;
        let mut native = Native::new(line);
        while let Some((tag, words)) = native.event() {
            if tag == "P" {
                status = words[0];
            }
            if tag == "r" {
                let (selector, mode, present) = profile.waits.pop_front().unwrap();
                let movie =
                    ((present || selector == -4) && !matches!(selector, -1 | -3)).then_some(status);
                assert_eq!(
                    u32::from(!wait_pending(mode, movie)),
                    words[0],
                    "Original MOVIEWAIT profile{} selector{} mode{}",
                    profile_index + 1,
                    selector,
                    mode
                );
                comparisons += 1;
            }
        }
        assert!(profile.waits.is_empty());
    }
    assert_eq!(comparisons, 645);
}
