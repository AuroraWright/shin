//! Direct original arithmetic and clip GPU-command comparison. This test does
//! not create a device, initialize audio or launch a window.
use super::*;
use std::{
    io::Write as _,
    process::{Command, Stdio},
};

fn word(tokens: &mut std::str::SplitWhitespace<'_>) -> u32 {
    tokens.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(tokens: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(tokens))
}

#[test]
#[ignore = "requires the original executable/font and direct native leaf oracle"]
fn higurashi_leaf_clips_and_block_culling_match_original() {
    let input = std::fs::read_to_string(
        std::env::var("SHIN_HIGURASHI_LEAF_CLIP_CULLING_INPUT")
            .unwrap_or_else(|_| "/tmp/higurashi-leaf-clip-culling-input.txt".into()),
    )
    .unwrap();
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LEAF_CLIP_CULLING").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copied_input = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copied_input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(input.lines().count(), output.lines().count());
    let mut clips = 0;
    let mut culling = 0;
    let mut mask_draws = 0;
    let mut project_words = 0;
    for (index, (request, response)) in input.lines().zip(output.lines()).enumerate() {
        let mut request = request.split_whitespace();
        let operation = request.next().unwrap();
        let mut response = response.split_whitespace();
        assert_eq!(response.next(), Some("G"));
        assert_eq!(word(&mut response), index as u32);
        if operation == "B" {
            let b = words::<4>(&mut request);
            let bounds = BlockBounds {
                x: b[0] as u16 as i16,
                y: b[1] as u16 as i16,
                width: b[2] as u16,
                height: b[3] as u16,
            };
            let matrix = Mat4::from_cols_array(&words::<16>(&mut request).map(f32::from_bits));
            let viewport = words::<4>(&mut request).map(f32::from_bits);
            assert_eq!(response.next(), Some("B"));
            assert_eq!(
                bounds.visible_in(matrix, viewport),
                word(&mut response) != 0,
                "profile{index}: visibility"
            );
            let original = words::<12>(&mut response);
            let actual: Vec<_> = bounds
                .projected_corners(matrix)
                .into_iter()
                .flatten()
                .map(f32::to_bits)
                .collect();
            assert_eq!(
                actual.as_slice(),
                &original,
                "profile{index}: native74ff8 projected corner bits"
            );
            culling += 1;
            project_words += original.len();
        } else {
            assert!(operation == "Q" || operation == "R");
            let values = words::<4>(&mut request).map(f32::from_bits);
            let equal = word(&mut request) != 0;
            let clip = if operation == "Q" {
                let mut properties = HigurashiProperties::from_seeds([0; 4]);
                for (i, value) in values.into_iter().enumerate() {
                    properties
                        .tweener_mut(44 + i)
                        .unwrap()
                        .fast_forward_to(value);
                }
                Clip::from_properties(&properties)
            } else {
                Clip(values)
            };
            assert_eq!(response.next(), Some("R"));
            assert_eq!(
                clip.0.map(f32::to_bits),
                words::<4>(&mut response),
                "profile{index}: native696d0 rectangle"
            );
            let mut clears = Vec::new();
            let mut depths = Vec::new();
            let mut stencils = Vec::new();
            let mut draws = Vec::new();
            loop {
                match response.next().unwrap() {
                    "C" => clears.push(words::<4>(&mut response)),
                    "Y" => depths.push(words::<2>(&mut response)),
                    "S" => stencils.push(words::<5>(&mut response)),
                    "D" => draws.push(words::<45>(&mut response)),
                    "A" => assert_eq!(
                        word(&mut response) != 0,
                        clip.active(),
                        "profile{index}: native6a994 gate"
                    ),
                    "E" => {
                        assert_eq!(words::<2>(&mut response), [clip.active() as u32; 2]);
                        break;
                    }
                    unknown => panic!("profile{index}: unknown{unknown}"),
                }
            }
            let request_stencil = [if equal { 6 } else { 4 }, 0, 0, 2, 0x7ffff];
            let default_stencil = [7, 0, 0, 0, 0xffff];
            if clip.active() {
                assert_eq!(
                    clears,
                    vec![[2, 0, 0, 0]],
                    "profile{index}: viewport-respecting depth-only clear"
                );
                assert_eq!(depths, vec![[7, 1], [1, 0], [7, 0]]);
                assert_eq!(
                    stencils,
                    vec![default_stencil, request_stencil, default_stencil]
                );
                assert_eq!(draws.len(), 1);
                let draw = draws[0];
                assert_eq!(&draw[..6], &[1, 0, 1, 4, 0, 0]);
                assert_eq!(&draw[6..8], &[7, 1]);
                assert_eq!(&draw[8..13], &default_stencil);
                assert_eq!(
                    &draw[13..29],
                    &Clip::native_projection().to_cols_array().map(f32::to_bits)
                );
                let vertices: Vec<_> = clip
                    .vertices()
                    .into_iter()
                    .flat_map(|v| {
                        let position = v.position;
                        let color = v.color;
                        [
                            position.x.to_bits(),
                            position.y.to_bits(),
                            position.z.to_bits(),
                            color.0,
                        ]
                    })
                    .collect();
                assert_eq!(
                    &draw[29..45],
                    vertices.as_slice(),
                    "profile{index}: native mask quad"
                );
                mask_draws += 1;
            } else {
                assert!(clears.is_empty() && draws.is_empty());
                assert_eq!(depths, vec![[7, 0], [7, 0]]);
                assert_eq!(stencils, vec![request_stencil, default_stencil]);
            }
            clips += 1;
        }
        assert!(request.next().is_none() && response.next().is_none());
    }
    assert_eq!(
        (clips, culling, mask_draws, project_words),
        (338, 2390, 300, 28680)
    );
    eprintln!(
        "Higurashi original leaf proof: {clips} clip profiles/{mask_draws} exact mask packets; {culling} projected-quad culling profiles/{project_words} corner words; no GPU/audio/windows"
    );
}
