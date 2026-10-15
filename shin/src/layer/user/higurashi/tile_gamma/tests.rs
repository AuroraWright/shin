//! Frozen original packets are the expectation; no native producer is compiled
//! or executed by the packet test. The separate ignored replay test requires
//! explicit executable/runner configuration and never silently skips.
use super::*;
use std::{fs, path::PathBuf};

use crate::layer::{
    higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
};
use glam::{Vec2, Vec3};
use shin_core::primitives::color::FloatColor4;

mod finite_pixels;
mod finite_sentinel;
mod owner;
mod partial_nan;
mod pixels;
mod sdk_active;
mod sdk_nonfinite;

pub(super) struct Case {
    pub(super) index: usize,
    pub(super) raw: [f32; 65],
    pub(super) properties: HigurashiProperties,
    pub(super) parent: HigurashiTransform,
    pub(super) tile: Tile,
    pub(super) requested_pass: PassKind,
    pub(super) original: Option<Original>,
}

pub(super) struct Original {
    pub(super) packet: [u32; 41],
    pub(super) input: [u32; 9],
    pub(super) before: [u32; 16],
    pub(super) after: [u32; 16],
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

pub(super) fn reference_directory() -> PathBuf {
    std::env::var_os("SHIN_HIGURASHI_TILE_GAMMA_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("reverse-engineering/higurashi/tile-gamma-native-capture"))
}

fn read_input(line: &str, index: usize) -> Case {
    let tokens: Vec<_> = line.split_ascii_whitespace().collect();
    assert_eq!(tokens.len(), 106, "profile{index} input length");
    assert_eq!(tokens[0], "T");
    let bits: Vec<_> = tokens[1..105]
        .iter()
        .map(|s| u32::from_str_radix(s, 16).unwrap())
        .collect();
    let values: Vec<_> = bits.iter().copied().map(f32::from_bits).collect();
    let raw: [f32; 65] = values[..65].try_into().unwrap();
    let mut properties = HigurashiProperties::from_seeds(bits[65..69].try_into().unwrap());
    for (index, &value) in raw.iter().enumerate() {
        properties
            .tweener_mut(index)
            .unwrap()
            .fast_forward_to(value);
    }
    for index in 0..4 {
        properties.oscillators[index].phase = values[69 + index];
    }
    Case {
        index,
        raw,
        properties,
        parent: HigurashiTransform {
            transform: Mat4::from_cols_array(values[73..89].try_into().unwrap()),
            camera: Vec3::from_array(values[89..92].try_into().unwrap()),
            inherited: Vec2::from_array(values[92..94].try_into().unwrap()),
            wobble: Vec2::from_array(values[94..96].try_into().unwrap()),
        },
        tile: Tile {
            color: FloatColor4::from_vec4(Vec4::from_array(values[96..100].try_into().unwrap())),
            rect: Vec4::from_array(values[100..104].try_into().unwrap()),
        },
        requested_pass: match tokens[105] {
            "0" => PassKind::Transparent,
            "1" => PassKind::Opaque,
            value => panic!("Unreviewed native pass{value}"),
        },
        original: None,
    }
}

pub(super) fn cases() -> Vec<Case> {
    let directory = reference_directory();
    let input = fs::read_to_string(directory.join("input.txt")).unwrap();
    let output = fs::read_to_string(directory.join("output.txt")).unwrap();
    assert_eq!(
        output.as_bytes(),
        fs::read(directory.join("output-repeat.txt")).unwrap(),
        "Original repeated output drift"
    );
    let mut cases: Vec<_> = input
        .lines()
        .enumerate()
        .map(|(index, line)| read_input(line, index))
        .collect();
    assert_eq!(cases.len(), 1024);
    assert_eq!(output.lines().count(), cases.len());
    for (index, line) in output.lines().enumerate() {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        assert_eq!(fields[0], "T");
        assert_eq!(fields[1].parse::<usize>().unwrap(), index);
        let words = |a, b| {
            fields[a..b]
                .iter()
                .map(|s| u32::from_str_radix(s, 16).unwrap())
                .collect::<Vec<_>>()
        };
        match fields[2] {
            "0" => assert_eq!(fields.len(), 3),
            "1" => {
                assert_eq!(fields.len(), 89);
                for (offset, name) in [(3, "P"), (45, "I"), (55, "B"), (72, "A")] {
                    assert_eq!(fields[offset], name);
                }
                cases[index].original = Some(Original {
                    packet: words(4, 45).try_into().unwrap(),
                    input: words(46, 55).try_into().unwrap(),
                    before: words(56, 72).try_into().unwrap(),
                    after: words(73, 89).try_into().unwrap(),
                });
            }
            other => panic!("Unreviewed original draw count{other}"),
        }
    }
    cases
}

pub(super) fn gamma(case: &Case) -> FullCanvas {
    FullCanvas::new(
        &case.tile,
        &case.properties.drawable_params(),
        case.raw[44..48].try_into().unwrap(),
        Clip::from_properties(&case.properties),
    )
    .unwrap()
}

pub(super) fn is_gamma(case: &Case) -> bool {
    let p = case.properties.drawable_params();
    p.fragment_shader.simplify(p.shader_param) == LayerFragmentShader::Gamma
}

fn native_blend(value: ColorBlendType) -> u32 {
    match value {
        ColorBlendType::Opaque => 1,
        ColorBlendType::Layer1 => 2,
        ColorBlendType::Layer2 => 3,
        ColorBlendType::HigurashiLayer3 => 4,
        value => panic!("Unreviewed older Tile blend{value:?}"),
    }
}

fn bgra(rgba: u32) -> u32 {
    (rgba & 0xff00_ff00) | ((rgba & 255) << 16) | ((rgba >> 16) & 255)
}

pub(super) fn check_packets(cases: &[Case]) {
    check_packets_expected(cases, (504, 188, 64, 20664));
}

pub(super) fn check_packets_expected(
    cases: &[Case],
    expected_counts: (usize, usize, usize, usize),
) {
    let (mut packets, mut gamma_packets, mut identity, mut words) = (0, 0, 0, 0);
    let mut gamma_opaque = 0;
    for case in cases {
        let p = case.properties.drawable_params();
        let submitted =
            case.properties.is_visible() && case.requested_pass == case.tile.pass_kind(&p);
        assert_eq!(submitted, case.original.is_some(), "profile{}", case.index);
        let Some(original) = &case.original else {
            continue;
        };
        let mut expected = Vec::with_capacity(41);
        let blend;
        if is_gamma(case) {
            let prepared = gamma(case);
            assert_eq!(prepared.pass_kind(), case.tile.pass_kind(&p));
            for vertex in prepared.vertices() {
                let position = vertex.position;
                expected.extend(position.to_array().map(f32::to_bits));
                expected.push(bgra(vertex.color.0));
            }
            blend = prepared.color_blend_type();
            let lanes = prepared.returned_lanes();
            let registers: [u32; 16] =
                std::array::from_fn(|i| if i % 4 == 0 { lanes[i / 4] } else { 0 });
            assert_eq!(original.before, registers, "before{}", case.index);
            assert_eq!(original.after, registers, "after{}", case.index);
            assert_eq!(original.input[8], 5);
            assert_eq!(prepared.vertices()[0].color.0 >> 24, 0);
            gamma_packets += 1;
            gamma_opaque += usize::from(case.requested_pass == PassKind::Opaque);
        } else {
            for vertex in case.tile.vertices(&p) {
                let position = vertex.position;
                expected.extend(position.to_array().map(f32::to_bits));
                expected.push(bgra(vertex.color.0));
            }
            blend = case.tile.color_blend_type(&p);
            assert_eq!(original.input[8], 0);
            identity += usize::from(case.raw[21] == 5.0);
        }
        let clamped = (case.tile.color.into_vec4() * p.color_multiplier.into_vec4())
            .min(Vec4::ONE)
            .max(Vec4::ZERO);
        assert_eq!(original.input[..4], clamped.to_array().map(f32::to_bits));
        assert_eq!(
            original.input[4..8],
            p.shader_param.to_array().map(f32::to_bits)
        );
        expected.extend(
            case.properties
                .composed_transform(&case.parent)
                .compute_native_tile_transform()
                .to_cols_array()
                .map(f32::to_bits),
        );
        expected.extend([1, native_blend(blend), 7, 0, 4, 0, 0, 2, 0x7ffff]);
        assert_eq!(expected.len(), 41);
        for (lane, (&actual, &native)) in expected.iter().zip(&original.packet).enumerate() {
            assert_eq!(actual, native, "profile{} lane{lane}", case.index);
        }
        packets += 1;
        words += expected.len();
    }
    assert_eq!((packets, gamma_packets, identity, words), expected_counts);
    assert!(
        gamma_opaque > 0,
        "pre-Gamma opaque routing was never exercised"
    );
    println!(
        "TileGamma fullcanvas:{}profiles/{packets}packets/{words}exactwords;{gamma_packets}Gamma+{identity}identity;preGamma opaqueGamma={gamma_opaque}",
        cases.len()
    );
}

#[test]
#[ignore = "Requires frozen updated TileGamma input/output corpus; CPU only, no native execution"]
fn higurashi_tile_gamma_fullcanvas_packets_match_original() {
    check_packets(&cases());
}

#[test]
#[ignore = "Explicit original CPU reexecution requires SHIN_HIGURASHI_NATIVE_TILE_GAMMA and SHIN_HIGURASHI_EXE"]
fn higurashi_tile_gamma_frozen_original_reexecution() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let directory = reference_directory();
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TILE_GAMMA").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = fs::read(directory.join("input.txt")).unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        fs::read(directory.join("output.txt")).unwrap()
    );
    assert_eq!(
        output.stderr,
        fs::read(directory.join("output-stderr.txt")).unwrap()
    );
    check_packets(&cases());
}

#[test]
fn higurashi_tile_gamma_packing_keeps_fp32_truncation_and_saturation() {
    for (input, byte) in [
        (-720.0, 0),
        (-0.0, 0),
        (0.001, 0),
        (0.25, 63),
        (0.75, 191),
        (1.0, 255),
        (360.0, 255),
    ] {
        assert_eq!(quantize_lane(input), byte);
    }
    assert_eq!(quantize_lane(f32::from_bits(CANONICAL_NAN_BITS)), 0);
}

fn reject(endpoints: [f32; 4], clip: Clip, rect: Vec4) {
    let tile = Tile {
        color: FloatColor4::WHITE,
        rect,
    };
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    properties.tweener_mut(21).unwrap().fast_forward_to(5.0);
    properties.tweener_mut(23).unwrap().fast_forward_to(500.0);
    assert!(FullCanvas::new(&tile, &properties.drawable_params(), endpoints, clip).is_err());
}

#[test]
fn higurashi_tile_gamma_fullcover_requires_exact_clip_and_gamma() {
    let rect = Vec4::new(-640.0, -720.0, 1280.0, 720.0);
    let endpoints = [0.0, 0.0, 1280.0, 720.0];
    let canonical = Clip([f32::from_bits(CANONICAL_NAN_BITS); 4]);
    for invalid in [
        [1.0, 0.0, 1280.0, 720.0],               // active finite clip
        [0.0, 0.0, 1279.0, 720.0],               // finite noncover
        [f32::from_bits(CANONICAL_NAN_BITS); 4], // same sentinel bits, authored NaNs
        [f32::from_bits(0x7fc1_2345), 0.0, 1280.0, 720.0],
        [f32::NEG_INFINITY, 0.0, f32::INFINITY, 720.0],
    ] {
        reject(invalid, Clip::from_endpoints(invalid), rect);
        reject(invalid, canonical, rect); // A supplied sentinel cannot bypass endpoints.
    }
    reject(endpoints, Clip([0.0, 0.0, 1280.0, 720.0]), rect);
    reject(endpoints, Clip([f32::from_bits(0xffc0_0000); 4]), rect);
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    properties.tweener_mut(21).unwrap().fast_forward_to(5.0);
    properties.tweener_mut(23).unwrap().fast_forward_to(500.0);
    let mut p = properties.drawable_params();
    let tile = Tile {
        color: FloatColor4::WHITE,
        rect: Vec4::new(0.0, 0.25, 1.0, 0.5),
    };
    let valid = FullCanvas::new(&tile, &p, endpoints, canonical).unwrap();
    let packed_color = valid.vertices()[0].color.0;
    assert_eq!(packed_color, 0x0000_3fbf);
    assert_eq!(valid.pass_kind(), PassKind::Opaque);
    // Nonfinite material/geometry is checked against the complete original
    // corpus in sdk_nonfinite. These are helper misuse guards only.
    p.fragment_shader = LayerFragmentShader::Default;
    assert!(FullCanvas::new(&tile, &p, endpoints, canonical).is_err());
    p.fragment_shader = LayerFragmentShader::Gamma;
    p.shader_param = Vec4::ONE;
    assert!(FullCanvas::new(&tile, &p, endpoints, canonical).is_err());
}

#[test]
#[ignore = "Requires the frozen960-profile partial-NaN input corpus; CPU guards only"]
fn higurashi_tile_gamma_fullcanvas_helper_rejects_every_frozen_partial_nan_profile() {
    let input = fs::read_to_string(
        root().join("reverse-engineering/higurashi/tile-gamma-partial-nan-capture/input.txt"),
    )
    .unwrap();
    let mut count = 0;
    for (index, line) in input.lines().enumerate() {
        let case = read_input(line, index);
        let mut p = case.properties.drawable_params();
        p.fragment_shader = LayerFragmentShader::Gamma;
        assert!(
            FullCanvas::new(
                &case.tile,
                &p,
                case.raw[44..48].try_into().unwrap(),
                Clip::from_properties(&case.properties),
            )
            .is_err(),
            "partial-NaN profile{index} unexpectedly admitted"
        );
        count += 1;
    }
    assert_eq!(count, 960);
}

#[test]
fn higurashi_tile_gamma_active_clip_requires_non_nan_getter_and_gamma() {
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    properties.tweener_mut(21).unwrap().fast_forward_to(5.0);
    properties.tweener_mut(23).unwrap().fast_forward_to(500.0);
    let tile = Tile {
        color: FloatColor4::WHITE,
        rect: Vec4::new(-640., -720., 1280., 720.),
    };
    let mut p = properties.drawable_params();
    for endpoints in [
        [10., 20., 310., 220.],
        [640., 360., 640., 360.],
        [f32::NEG_INFINITY, 20., 310., 220.],
        [-f32::MAX, 20., f32::MAX, 220.],
    ] {
        assert!(super::ActiveClip::new(&tile, &p, endpoints).is_ok());
    }
    for endpoints in [[f32::NAN, 20., 310., 220.], [0., 0., 1280., 720.]] {
        assert!(super::ActiveClip::new(&tile, &p, endpoints).is_err());
    }
    p.fragment_shader = LayerFragmentShader::Default;
    assert!(super::ActiveClip::new(&tile, &p, [10., 20., 310., 220.]).is_err());
}
