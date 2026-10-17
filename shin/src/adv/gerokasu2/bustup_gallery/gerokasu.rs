//! Gerokasu BustupMode catalog (see gerokasu/BUPMODE.md). The executable
//! hard-codes it (character table 0x17b930, backgrounds 0x17b970) instead of
//! reading scenario character-box metadata, and adds a global size row.
use super::catalog::{Background, Catalog, Character, Outfit, Pose};

/// `[dress_param, entry S, M, L, XL, expression base, expression count]`.
type PoseRow = [i32; 7];

const FUUKA: &[&[PoseRow]] = &[
    &[[0, 1, 2, 3, 4, 0, 46], [0, 6, 7, 8, 9, 100, 42]],
    &[[1, 1, 2, 3, 4, 0, 45], [1, 6, 7, 8, 9, 100, 42]],
    &[[2, 11, 12, 13, 14, 300, 26]],
];
const KARIN: &[&[PoseRow]] = &[
    &[[0, 16, 17, 18, 19, 0, 45], [0, 21, 22, 23, 24, 100, 43]],
    &[[1, 16, 17, 18, 19, 0, 45], [1, 21, 22, 23, 24, 100, 43]],
    &[[2, 26, 27, 28, 29, 300, 25]],
];
const MINA: &[&[PoseRow]] = &[
    &[
        [0, 31, 32, 33, 34, 0, 46],
        [0, 36, 37, 38, 39, 100, 45],
        [0, 41, 42, 43, 44, 200, 45],
    ],
    &[
        [1, 31, 32, 33, 34, 0, 46],
        [1, 36, 37, 38, 39, 100, 45],
        [1, 41, 42, 43, 44, 200, 45],
    ],
    &[[2, 46, 47, 48, 49, 300, 25]],
];
const DOREMI: &[&[PoseRow]] = &[
    &[[0, 51, 52, 53, 54, 0, 39], [0, 56, 57, 58, 59, 100, 43]],
    &[[1, 51, 52, 53, 54, 0, 39], [1, 56, 57, 58, 59, 100, 43]],
    &[[2, 61, 62, 63, 64, 300, 27]],
];
pub const CHARACTERS: [&[&[PoseRow]]; 4] = [FUUKA, KARIN, MINA, DOREMI];

pub const BACKGROUNDS: [&str; 18] = [
    "/picture/bg00a.pic",
    "/picture/bg01a.pic",
    "/picture/bg02a.pic",
    "/picture/bg02b.pic",
    "/picture/bg03a.pic",
    "/picture/bg03b.pic",
    "/picture/bg03o.pic",
    "/picture/bg03r.pic",
    "/picture/bg04a.pic",
    "/picture/bg04b.pic",
    "/picture/bg04o.pic",
    "/picture/bg04r.pic",
    "/picture/bg05a.pic",
    "/picture/bg05b.pic",
    "/picture/bg05o.pic",
    "/picture/bg05r.pic",
    "/picture/bg06a.pic",
    "/picture/bg07b.pic",
];
pub const SIZES: usize = 4;
/// Ctor 0x36dac starts on 中 (M).
pub const INITIAL_SIZE: usize = 1;
/// Table 0x122910: property 2 for a reloaded bustup, per size.
pub const SIZE_Z: [f32; SIZES] = [1538., 1000., 666., 500.];
/// Property 2 for every background (0x3dfd0).
pub const BACKGROUND_Z: f32 = 333.;
pub const BACKGROUND_LAYER: u8 = 0;
pub const BUSTUP_LAYER: u8 = 1;
/// 0x3b560 plays bgm07 (id 6), not Gerokasu2's 8.
pub const BGM: i32 = 6;

/// The shared selection hierarchy; expressions are plain indices.
pub fn catalog() -> Catalog {
    Catalog {
        backgrounds: (0..BACKGROUNDS.len() as u16)
            .map(|primary| Background {
                primary,
                secondary: None,
            })
            .collect(),
        characters: CHARACTERS
            .iter()
            .map(|dresses| Character {
                outfits: dresses
                    .iter()
                    .map(|poses| Outfit {
                        poses: poses
                            .iter()
                            .map(|pose| Pose {
                                expressions: (0..pose[6] as u16).collect(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Native 0x39df0: scenario bustup entry, `%DRESS%` parameter and expression
/// number. Expression 45 of 風華's first uniform pose is the red-eye uniform.
pub fn request(
    character: usize,
    size: usize,
    dress: usize,
    pose: usize,
    expression: usize,
) -> [i32; 3] {
    let row = CHARACTERS[character][dress][pose];
    let entry = row[1 + size];
    if expression == 45 && character == 0 && dress == 0 && pose == 0 {
        [entry, 3, 0]
    } else {
        [entry, row[0], row[5] + expression as i32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_counts_and_requests_match_native_tables() {
        let catalog = catalog();
        assert_eq!(catalog.backgrounds.len(), 18);
        assert_eq!(
            catalog
                .characters
                .iter()
                .map(|c| c.outfits.iter().map(|o| o.poses.len()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [vec![2, 2, 1], vec![2, 2, 1], vec![3, 3, 1], vec![2, 2, 1]]
        );
        assert_eq!(
            catalog.characters[0].outfits[0].poses[0].expressions.len(),
            46
        );
        // M uniform pose 1 of 風華, then XL collar pose 2 of 水無.
        assert_eq!(request(0, 1, 0, 0, 3), [2, 0, 3]);
        assert_eq!(request(2, 3, 1, 2, 7), [44, 1, 207]);
        assert_eq!(request(0, 0, 0, 0, 45), [1, 3, 0]);
        assert_eq!(request(0, 0, 1, 0, 44), [1, 1, 44]);
    }
}
