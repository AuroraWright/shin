//! NEW actual6aaec RGB+alpha combinations across all six operations and both
//! root/plane callers. The old174/178 native captures remain input checkpoints.
//! Expectations are newly captured original D113, not another Rust formula.
use super::*;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(serde::Deserialize)]
struct Case {
    index: usize,
    name: String,
    scope: u32,
    operation: u32,
    program: u32,
    values: Vec<i32>,
    profile_class: String,
    packet: Option<Vec<u32>>,
    tile_packet: Vec<u32>,
    allocated: u32,
    retired: u32,
}
#[derive(serde::Deserialize)]
struct Original {
    cases: Vec<Case>,
}
fn reference_directory() -> PathBuf {
    std::env::var("SHIN_HIGURASHI_FRAMEBUFFER_COLOR_COMBINATIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../reverse-engineering/higurashi/framebuffer-color-combinations-capture")
        })
}
#[test]
fn higurashi_framebuffer_raw_identity_and_emitted_premultiplied_rgba_match_original_combinations() {
    let original: Original =
        serde_json::from_slice(&std::fs::read(reference_directory().join("packets.json")).unwrap())
            .unwrap();
    assert_eq!(original.cases.len(), 96);
    let mut programs = BTreeMap::<u32, usize>::new();
    let mut emitted = 0;
    let mut identities = 0;
    let mut words = 0;
    for (index, case) in original.cases.into_iter().enumerate() {
        assert_eq!(case.index, index);
        assert_eq!(case.values.len(), 65);
        assert!(case.scope <= 1);
        assert_eq!(case.program, [6, 9, 7, 8, 10, 11][case.operation as usize]);
        assert_eq!(case.allocated, case.retired);
        assert_eq!(case.tile_packet.len(), 113);
        assert_eq!(&case.tile_packet[3..5], [1, 1]);
        let mut properties = HigurashiProperties::from_seeds([0; 4]);
        for (i, value) in case.values.iter().copied().enumerate() {
            properties
                .tweener_mut(i)
                .unwrap()
                .fast_forward_to(value as f32)
        }
        let actual = Packet::new(&properties);
        assert_eq!(
            actual.is_some(),
            case.packet.is_some(),
            "{} raw identity class{}",
            case.name,
            case.profile_class
        );
        if let (Some(packet), Some(native)) = (actual, case.packet) {
            assert_eq!(native.len(), 113);
            assert_eq!(native[3], packet.program, "{}", case.name);
            assert_eq!(native[3], case.program);
            assert_eq!(native[4], 1);
            assert_eq!(&native[9..12], [1, 3, 48]);
            assert_eq!(&native[21..24], [64, 16, 16]);
            assert_eq!(
                packet
                    .projection
                    .to_cols_array()
                    .map(f32::to_bits)
                    .as_slice(),
                &native[25..41],
                "{} projection",
                case.name
            );
            assert_eq!(
                packet.color.to_array().map(f32::to_bits).as_slice(),
                &native[41..45],
                "{} emitted material RGBA",
                case.name
            );
            assert_eq!(
                packet.param.to_array().map(f32::to_bits).as_slice(),
                &native[57..61],
                "{} params",
                case.name
            );
            let vertices = packet
                .vertices
                .iter()
                .flat_map(|vertex| {
                    // PosTexVertex can be packed; copy before calling vector APIs.
                    let position = vertex.position;
                    let texture = vertex.texture_position;
                    position
                        .to_array()
                        .into_iter()
                        .chain(texture.to_array())
                        .map(f32::to_bits)
                })
                .collect::<Vec<_>>();
            assert_eq!(vertices, &native[89..101], "{} geometry", case.name);
            *programs.entry(packet.program).or_default() += 1;
            emitted += 1;
            words += 36;
        } else {
            identities += 1
        }
    }
    assert_eq!((emitted, identities, words), (76, 20, 2736));
    assert_eq!(
        programs,
        BTreeMap::from([(6, 12), (9, 12), (7, 12), (8, 12), (10, 16), (11, 12)])
    );
    println!(
        "higurashi_framebuffer_color_combinations=96profiles/76exactPackets/20rawIdentitySkips/2736words;all6operations;rootAndPlane;RGBplusAlpha;clampedAlpha;originalD113"
    );
}
