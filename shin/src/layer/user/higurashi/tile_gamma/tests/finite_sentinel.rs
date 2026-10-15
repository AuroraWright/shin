//! Finite full-cover native corpus adapter. The original four raw captures and
//! all input bytes are immutable expectations; no native program runs here.
//! C records are getter-only controls and NEVER become reference Tile draws.
use super::{Case, Original, check_packets_expected, is_gamma, read_input, root};
use crate::layer::user::higurashi::{leaf_clip::Clip, tile_gamma::FullCanvas};
use shin_render::LayerFragmentShader;
use std::fs;

const CANONICAL: [u32; 4] = [0x7fc0_0000; 4];
const INPUT_BYTES: usize = 368_480;
const INPUT_FNV: u64 = 0x2143_c3ae_9661_bb60;
const OUTPUT_BYTES: usize = 245_626;
const OUTPUT_FNV: u64 = 0x8734_8a78_26e0_d451;
const STDERR_BYTES: usize = 194;
const STDERR_FNV: u64 = 0x5fa1_b54f_c152_a708;

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn pinned(bytes: &[u8], length: usize, expected: u64, label: &str) {
    assert_eq!(bytes.len(), length, "{label}:frozen length");
    assert_eq!(fnv(bytes), expected, "{label}:frozen FNV");
}
fn group<const N: usize>(tokens: &[&str], at: &mut usize, label: &str) -> [u32; N] {
    assert_eq!(tokens[*at], label);
    let words = std::array::from_fn(|i| {
        let value = tokens[*at + 1 + i];
        assert_eq!(value.len(), 8, "{label}:word width");
        u32::from_str_radix(value, 16).unwrap()
    });
    *at += N + 1;
    words
}
fn scalar_lanes(words: [u32; 4]) -> [u32; 16] {
    std::array::from_fn(|i| if i % 4 == 0 { words[i / 4] } else { 0 })
}

fn load() -> (Vec<Case>, Vec<Case>) {
    let base = root().join("reverse-engineering/higurashi");
    let input =
        fs::read(base.join("tile-gamma-finite-sentinel-candidate-inputs/input.txt")).unwrap();
    pinned(&input, INPUT_BYTES, INPUT_FNV, "finite input");
    let capture = base.join("tile-gamma-finite-sentinel-parent-capture-initial");
    let mut output = None;
    for name in ["O0-run1", "O0-run2", "O3-run1", "O3-run2"] {
        let raw = fs::read(capture.join(format!("{name}.stdout.txt"))).unwrap();
        pinned(&raw, OUTPUT_BYTES, OUTPUT_FNV, name);
        let stderr = fs::read(capture.join(format!("{name}.stderr.txt"))).unwrap();
        pinned(&stderr, STDERR_BYTES, STDERR_FNV, name);
        if let Some(first) = &output {
            assert_eq!(first, &raw, "native compiler/repeat drift at{name}");
        } else {
            output = Some(raw);
        }
    }
    let input = String::from_utf8(input).unwrap();
    let output = String::from_utf8(output.unwrap()).unwrap();
    assert_eq!(input.lines().count(), 392);
    assert_eq!(output.lines().count(), 392);
    let mut cases = Vec::new();
    let mut controls = Vec::new();
    let mut cache = [0; 9];
    let mut draw_count = 0;
    for (index, (input, output)) in input.lines().zip(output.lines()).enumerate() {
        let fields: Vec<_> = output.split_ascii_whitespace().collect();
        let operation = fields[0];
        assert_eq!(fields[1].parse::<usize>().unwrap(), index);
        assert!(matches!(fields[2], "0" | "1"));
        let draws = fields[2] == "1";
        let mut at = 3;
        let clip = group::<4>(&fields, &mut at, "C");
        assert_eq!(group::<4>(&fields, &mut at, "N"), CANONICAL);
        let after_cache = group::<9>(&fields, &mut at, "F");
        let fp = group::<2>(&fields, &mut at, "E");
        assert_eq!(fp[0], 0, "native fixture FPCR");
        // Observed ordered-process history, not a per-operation FP contract.
        assert_eq!(fp[1], if index < 5 { 0x10 } else { 0x0800_0011 });
        match operation {
            "T" => {
                assert!(input.starts_with("T "));
                let mut case = read_input(input, index);
                assert!(case.raw[44..48].iter().all(|v| v.is_finite()));
                assert_eq!(clip, CANONICAL);
                assert_eq!(
                    Clip::from_properties(&case.properties).0.map(f32::to_bits),
                    clip
                );
                if draws {
                    let packet = group::<41>(&fields, &mut at, "P");
                    let material = group::<9>(&fields, &mut at, "I");
                    let before = group::<16>(&fields, &mut at, "B");
                    let after = group::<16>(&fields, &mut at, "A");
                    assert_eq!(group::<16>(&fields, &mut at, "Q"), scalar_lanes(CANONICAL));
                    assert_eq!(fields.len(), 129);
                    assert_eq!(packet[32], 1);
                    assert_eq!(packet[34..], [7, 0, 4, 0, 0, 2, 0x7ffff]);
                    cache = [1, packet[33], 7, 0, 7, 0, 0, 0, 0xffff];
                    case.original = Some(Original {
                        packet,
                        input: material,
                        before,
                        after,
                    });
                    draw_count += 1;
                } else {
                    assert_eq!(fields.len(), 26);
                }
                cases.push(case);
            }
            "C" => {
                assert!(input.starts_with("C "));
                assert!(!draws);
                assert_eq!(fields.len(), 43);
                assert!(clip.iter().all(|&v| f32::from_bits(v).is_finite()));
                assert_eq!(group::<16>(&fields, &mut at, "Q"), scalar_lanes(clip));
                // Reuse the prepared-state decoder with its T-only assertion.
                // Only this local transport tag changes; original bytes stay
                // pinned. This creates NO original/drawn Tile for a C record.
                let prepared = format!("T {}", input.split_once(' ').unwrap().1);
                let control = read_input(&prepared, index);
                assert!(control.original.is_none());
                assert!(control.raw[44..48].iter().all(|v| v.is_finite()));
                assert_eq!(
                    Clip::from_properties(&control.properties)
                        .0
                        .map(f32::to_bits),
                    clip
                );
                controls.push(control);
            }
            other => panic!("Unreviewed finite native operation{other}"),
        }
        assert_eq!(at, fields.len(), "profile{index}:trailing native words");
        assert_eq!(after_cache, cache, "profile{index}:native cache history");
    }
    assert_eq!((cases.len(), controls.len(), draw_count), (384, 8, 192));
    (cases, controls)
}

pub(super) fn cases() -> Vec<Case> {
    load().0
}

#[test]
#[ignore = "Frozen finite-cover392 native records; CPU only, no native execution"]
fn higurashi_tile_gamma_finite_sentinel_packets_match_original() {
    let (cases, controls) = load();
    assert_eq!(cases.iter().filter(|c| c.original.is_none()).count(), 192);
    assert_eq!(
        cases
            .iter()
            .filter(|c| c.original.is_some() && is_gamma(c))
            .count(),
        64
    );
    assert_eq!(
        cases
            .iter()
            .filter(|c| c.original.is_some() && c.raw[21] == 5. && !is_gamma(c))
            .count(),
        64
    );
    check_packets_expected(&cases, (192, 64, 64, 7872));
    for control in controls {
        let mut material = control.properties.drawable_params();
        material.fragment_shader = LayerFragmentShader::Gamma;
        // C used nonidentity parameters already. Force only shader selection
        // so these finite active controls test the production Gamma guard.
        assert_eq!(
            material.fragment_shader.simplify(material.shader_param),
            LayerFragmentShader::Gamma
        );
        assert!(
            FullCanvas::new(
                &control.tile,
                &material,
                control.raw[44..48].try_into().unwrap(),
                Clip::from_properties(&control.properties),
            )
            .is_err(),
            "getter-only control{} unexpectedly admitted",
            control.index
        );
    }
    println!(
        "TileGamma finite sentinel:384T/192packets/7872exactwords/64Gamma/64identity/192pass-skips;8getter-only active controls rejected;native canonical C/N/Q and four compiler/repeat captures pinned"
    );
}
