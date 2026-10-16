use crate::title::bustup::menu::Controls;
use super::*;
use shin_core::{
    format::scenario::{
        info::{BustupId, CharacterBoxSegment, PictureId},
        instruction_elements::FromNumber,
    },
    time::Ticks,
};
fn catalog() -> BustupCatalog {
    use CharacterBoxSegment::*;
    BustupCatalog::new(&vec![
        Background {
            primary_picture_id: PictureId::from_number(0),
            secondary_picture_id_offset: 0,
        },
        EndDefinition,
        Bustup {
            bustup_id: BustupId::from_number(0),
        },
        EndDefinition,
    ])
    .unwrap()
}
#[test]
fn original_draw_order_icons_labels_glyphs_and_dimmed_arrows() {
    let c = catalog();
    let m = BustupMenu::new(&c);
    let plan = artwork(&m, &c, 1.0);
    assert_eq!(plan[0].name, "icon");
    assert_eq!(plan[0].rect, vec4(1504.0, 248.0, 1648.0, 392.0));
    assert_eq!(plan[1].name, "char01");
    assert_eq!(plan[2].name, "icon");
    assert_eq!(plan[2].uv, vec4(0.0, 0.5, 0.5, 1.0));
    assert_eq!(plan[3].name, "text");
    assert_eq!(plan[3].rect, vec4(1504.0, 356.0, 1648.0, 388.0));
    let arrows = plan
        .iter()
        .filter(|d| d.name == "arrow")
        .collect::<Vec<_>>();
    assert_eq!(arrows.len(), 12); // Focused row has an overlay after each arrow.
    assert_eq!(arrows[0].tint, UnormColor::from_rgba(102, 102, 102, 255));
    assert_eq!(arrows[0].flash, Some(0.0));
    assert_eq!(arrows[1].uv, vec4(0.0, 0.5, 0.5, 1.0));
    let labels = plan.iter().filter(|d| d.name == "text").collect::<Vec<_>>();
    assert_eq!(labels.len(), 5);
    for (label, index) in labels.into_iter().zip([0, 2, 3, 4, 5]) {
        assert_eq!(label.uv.y, index as f32 / 6.0);
    }
    let close = plan.iter().find(|d| d.name == "close").unwrap();
    assert_eq!(close.rect, vec4(1764.0, 32.0, 1848.0, 116.0));
    let help = plan
        .iter()
        .filter(|d| d.name == "help_bupmode")
        .collect::<Vec<_>>();
    assert_eq!(
        help.iter().map(|d| d.rect.x).collect::<Vec<_>>(),
        [1792.0, 1556.0, 1286.0, 970.0]
    );
    assert!(
        help.iter()
            .all(|d| d.rect.y == 1012.0 && d.rect.w == 1060.0)
    );
}
#[test]
fn native_counter_centering_and_half_controls_slide() {
    let mut glyphs = Vec::new();
    number_glyphs(&mut glyphs, 9, 23, 1520.0, 392.0);
    assert_eq!(glyphs.len(), 5);
    assert_eq!(glyphs[0].rect, vec4(1535.0, 428.0, 1553.0, 468.0));
    assert_eq!(glyphs[2].uv.x, 320.0 / 352.0);
    assert_eq!(glyphs[0].tint, UnormColor::from_rgba(204, 187, 170, 255));
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    let mut eligible = true;
    m.controller(
        &c,
        &mut eligible,
        false,
        Controls {
            confirm: true,
            ..Default::default()
        },
    );
    m.update(Ticks::from_f32(15.0));
    let shifted = artwork(&m, &c, 0.5);
    assert!((shifted[0].rect.x - (1504.0 + 500.0 / 2.0f32.sqrt())).abs() < 2e-4);
    assert_eq!(
        shifted.iter().find(|d| d.name == "close").unwrap().rect.y,
        -43.0
    );
    assert_eq!(
        shifted
            .iter()
            .find(|d| d.name == "help_bupmode")
            .unwrap()
            .rect
            .y,
        1056.0
    );
    let hidden = artwork(&m, &c, 0.0);
    assert!(
        hidden
            .iter()
            .all(|d| !matches!(d.name.as_str(), "close" | "hidden" | "help_bupmode" | "key"))
    );
}

#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_original_bustup_artwork_assets_and_draw_helpers() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let mut read = |name: &str| {
        let mut b = Vec::new();
        rom.open_file(rom.find_file(name).unwrap())
            .unwrap()
            .read_to_end(&mut b)
            .unwrap();
        b
    };
    let atlas =
        shin_core::format::texture_archive::read_texture_archive(&read("/bupmode.txa")).unwrap();
    let scenario = Scenario::new(read("/main.snr").into()).unwrap();
    let c = BustupCatalog::new(&scenario.info_tables().character_box_info).unwrap();
    for (name, size) in [
        ("icon", (288, 288)),
        ("arrow", (160, 160)),
        ("text", (144, 192)),
        ("num", (352, 40)),
        ("help_bupmode", (320, 192)),
        ("key", (384, 96)),
        ("close", (84, 84)),
        ("hidden", (84, 84)),
    ] {
        assert_eq!(atlas.textures[atlas.name_to_index[name]].dimensions(), size);
    }
    let mut m = BustupMenu::new(&c);
    let input = |m: &mut BustupMenu, controls| {
        let mut eligible = true;
        m.controller(&c, &mut eligible, false, controls);
    };
    for ci in 0..c.characters.len() {
        if ci > 0 {
            input(
                &mut m,
                Controls {
                    right: true,
                    ..Default::default()
                },
            );
        }
        let plan = artwork(&m, &c, 1.0);
        assert_eq!(plan[1].name, format!("char{:02}", ci + 1));
        for item in plan {
            assert!(
                atlas.name_to_index.contains_key(&item.name),
                "{}",
                item.name
            );
        }
    }
    for _ in 0..4 {
        input(
            &mut m,
            Controls {
                down: true,
                ..Default::default()
            },
        );
    }
    for bi in 0..c.backgrounds.len() {
        if bi > 0 {
            input(
                &mut m,
                Controls {
                    right: true,
                    ..Default::default()
                },
            );
        }
        let plan = artwork(&m, &c, 1.0);
        let name = format!("bg{:03}", bi + 1);
        assert!(plan.iter().any(|d| d.name == name));
        assert_eq!(
            atlas.textures[atlas.name_to_index[&name]].dimensions(),
            (144, 144)
        );
    }
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, expected) in [
        (0x389c0, 0x91030268),
        (0x38a10, 0x9400e1b4),
        (0x38b24, 0x9107e260),
        (0x38b30, 0x94020a5c),
        (0x38b78, 0x9400023a),
        (0x38c90, 0x940002e0),
        (0x38ccc, 0x321f03e2),
        (0x38da0, 0x9400029c),
        (0x38dd8, 0x320007e2),
        (0x38e90, 0x94000260),
        (0x38ec8, 0x321e03e2),
        (0x38f74, 0x9400029f),
        (0x38fa4, 0x528000a2),
        (0x39038, 0x9103c274),
        (0x390ac, 0x1e210800),
        (0x39188, 0x94023762),
        (0x39240, 0x94023734),
        (0x39258, 0x1e210803),
        (0x39280, 0x94000218),
        (0x39294, 0x94000213),
        (0x392a8, 0x9400020e),
        (0x392bc, 0x94000209),
        (0x39370, 0xbc74d90b),
        (0x3938c, 0x1e20196a),
        (0x393d4, 0x910043e0),
        (0x3948c, 0x11000662),
        (0x395b0, 0xbc74d901),
        (0x395fc, 0x531b6a68),
        (0x39608, 0xbd4a1d01),
        (0x39610, 0x1e211801),
        (0x39704, 0x1e2202a0),
        (0x39748, 0xbd490d00),
        (0x39794, 0x940235df),
        (0x397f0, 0x94023588),
        (0x3983c, 0x7100203f),
        (0x39874, 0x7100227f),
        (0x39894, 0x11000682),
        (0x39898, 0x11000663),
        (0x39960, 0x1e2f2802),
        (0x3997c, 0x1e2c1800),
        (0x39980, 0x1e282802),
        (0x3999c, 0x1e261000),
        (0x39a1c, 0x11000662),
        (0x39b30, 0xbd400e80),
        (0x39b98, 0xbd444500),
        (0x39bf4, 0x97ffb5cb),
        (0x39c00, 0xbd44d100),
        (0x39c9c, 0xbd44d100),
        (0x39d24, 0x1e349000),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup draw{a:x}"
        );
    }
    for (a, expected) in [
        (0x159a78, 0x43900000),
        (0x159af0, 0x3e99999a),
        (0x15961c, 0xc2900000),
        (0x159950, 0xc2100000),
        (0x159a1c, 0x43400000),
        (0x159540, 0x3e2aaaab),
        (0x15990c, 0x3ecccccd),
        (0x159544, 0x42600000),
        (0x159824, 0x42100000),
        (0x15997c, 0x42200000),
        (0x15975c, 0x43b00000),
        (0x159858, 0x3d51745d),
        (0x1594fc, 0x3f4ccccd),
        (0x159408, 0x3f3bbbbc),
        (0x1593ec, 0x3f2aaaab),
        (0x159a20, 0x43160000),
        (0x159434, 0xc3160000),
        (0x1597ac, 0x42a80000),
        (0x159a24, 0xc2b00000),
        (0x1599d0, 0x44ea0000),
        (0x159584, 0x42400000),
        (0x159444, 0x43a00000),
        (0x1594d0, 0xc2380000),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup draw constant{a:x}"
        );
    }
    for (i, expected) in [32.0f32, 48.0, 144.0, 112.0].into_iter().enumerate() {
        assert_eq!(
            f32::from_le_bytes(
                exe[0x164cc0 + 256 + i * 4..0x164cc0 + 260 + i * 4]
                    .try_into()
                    .unwrap()
            ),
            expected
        );
    }
    for (i, row) in [
        [1i32, -1, 0, 214],
        [4, -1, 48, 80],
        [3, -1, 96, 180],
        [7, 2, 144, 214],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, v) in row.into_iter().enumerate() {
            let a = 0x165d64 + 256 + i * 16 + j * 4;
            assert_eq!(i32::from_le_bytes(exe[a..a + 4].try_into().unwrap()), v);
        }
    }
}
