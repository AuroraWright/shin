use super::*;
use CharacterBoxSegment::*;
fn b(id: i32) -> CharacterBoxSegment {
    Bustup {
        bustup_id: BustupId::from_number(id),
    }
}
fn bg(primary: i32, offset: u16) -> CharacterBoxSegment {
    CharacterBoxSegment::Background {
        primary_picture_id: PictureId::from_number(primary),
        secondary_picture_id_offset: offset,
    }
}
#[test]
fn groups_preserve_duplicate_ids_and_remember_each_parent_selection() {
    let catalog = BustupCatalog::new(&vec![
        bg(3, 0),
        bg(4, 2),
        EndDefinition,
        b(10),
        b(10),
        EndExpressionGroup,
        b(11),
        b(12),
        EndPoseGroup,
        b(13),
        b(14),
        EndDefinition,
        b(20),
        EndDefinition,
    ])
    .unwrap();
    assert_eq!(
        catalog.backgrounds,
        [
            super::Background {
                primary: PictureId::from_number(3),
                secondary: None
            },
            super::Background {
                primary: PictureId::from_number(4),
                secondary: Some(PictureId::from_number(6))
            }
        ]
    );
    assert_eq!(catalog.characters.len(), 2);
    let mut state = BustupSelection::new(&catalog);
    assert!(state.select_expression(&catalog, 1));
    assert_eq!(state.indices(), (0, 0, 0, 1));
    assert!(state.select_pose(1));
    assert!(state.select_expression(&catalog, 1));
    assert_eq!(state.bustup(&catalog), BustupId::from_number(12));
    assert!(state.select_outfit(1));
    assert!(state.select_expression(&catalog, 1));
    assert_eq!(state.bustup(&catalog), BustupId::from_number(14));
    assert!(state.select_character(1));
    assert_eq!(state.indices(), (1, 0, 0, 0));
    assert!(!state.select_expression(&catalog, 1));
    assert!(!state.select_character(2));
    assert!(!state.select_outfit(1));
    assert!(!state.select_pose(1));
    assert!(state.select_character(0));
    assert_eq!(state.indices(), (0, 1, 0, 1));
    assert!(state.select_outfit(0));
    assert_eq!(state.indices(), (0, 0, 1, 1));
    assert!(state.select_pose(0));
    assert_eq!(state.indices(), (0, 0, 0, 1));
    assert!(state.select_background(&catalog, 1));
    assert!(!state.select_background(&catalog, 2));
    assert_eq!(state.background(&catalog), &catalog.backgrounds[1]);
}
#[test]
fn malformed_group_boundaries_are_rejected() {
    for segments in [
        vec![],
        vec![bg(0, 0)],
        vec![bg(0, 0), EndDefinition, EndPoseGroup],
        vec![
            bg(0, 0),
            EndDefinition,
            b(1),
            EndExpressionGroup,
            EndDefinition,
        ],
        vec![bg(0, 0), EndDefinition, EndExpressionGroup],
        vec![
            bg(0, 0),
            EndDefinition,
            b(1),
            EndExpressionGroup,
            EndDefinition,
        ],
    ] {
        assert!(BustupCatalog::new(&segments).is_err());
    }
}
#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_original_bustup_hierarchy_assets_and_selection_layout() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();

    let segments = &scenario.info_tables().character_box_info;
    let catalog = BustupCatalog::new(segments).unwrap();
    let mut state = BustupSelection::new(&catalog);
    let (mut outfits, mut poses, mut expressions) = (0, 0, 0);
    for (c, character) in catalog.characters.iter().enumerate() {
        assert!(state.select_character(c));
        assert_eq!(state.indices(), (c, 0, 0, 0));
        outfits += character.outfits.len();
        for (o, outfit) in character.outfits.iter().enumerate() {
            assert!(state.select_outfit(o));
            poses += outfit.poses.len();
            for (p, pose) in outfit.poses.iter().enumerate() {
                assert!(state.select_pose(p));
                expressions += pose.expressions.len();
                for (e, id) in pose.expressions.iter().enumerate() {
                    assert!(state.select_expression(&catalog, e));
                    assert_eq!(state.bustup(&catalog), *id);
                    assert!(
                        rom.find_file(&scenario.info_tables().bustup_info(*id).path())
                            .is_ok()
                    );
                }
            }
        }
    }
    for bg in &catalog.backgrounds {
        for id in [Some(bg.primary), bg.secondary].into_iter().flatten() {
            assert!(
                rom.find_file(&scenario.info_tables().picture_info(id).path())
                    .is_ok()
            );
        }
    }
    assert!(rom.find_file("/bupmode.txa").is_ok());
    assert_eq!(
        (
            catalog.backgrounds.len(),
            catalog.characters.len(),
            outfits,
            poses,
            expressions
        ),
        (219, 65, 98, 559, 8195)
    );
    let flat = catalog
        .characters
        .iter()
        .flat_map(|c| c.outfits.iter())
        .flat_map(|o| o.poses.iter())
        .flat_map(|p| p.expressions.iter().copied())
        .collect::<Vec<_>>();
    let original = segments
        .iter()
        .filter_map(|s| {
            if let CharacterBoxSegment::Bustup { bustup_id } = s {
                Some(*bustup_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(flat, original);
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, expected) in [
        (0x10dcc0, 0x97fcbddc),
        (0x40258, 0x97ffde86),
        (0xea3f0, 0x9103a000),
        (0x37f34, 0xf9006aa0),
        (0x37f38, 0xa941a408),
        (0x37f6c, 0x9400155d),
        (0x38030, 0x940015d4),
        (0x380d0, 0x94001644),
        (0x37d2c, 0xb9025abf),
        (0x37eec, 0xb9027abf),
        (0x3ad90, 0xb9425808),
        (0x3ad94, 0xf941300a),
        (0x3ada4, 0x8b091549),
        (0x3ada8, 0x7940012a),
        (0x3adb0, 0x8b0a1529),
        (0x3adb4, 0x7940012b),
        (0x3adc8, 0x786b7929),
        (0x3add4, 0xf9400d8c),
        (0x3addc, 0xf8686988),
        (0x3ade0, 0xf86a6908),
        (0x3ade8, 0xf86a6908),
        (0x3adec, 0x78697901),
        (0x3adf0, 0x9402bd28),
        (0x3c840, 0x321e0fe1),
        (0x3c844, 0x94009997),
        (0x3c854, 0x321e0fe1),
        (0x3c858, 0x97ffdcf6),
        (0x3c884, 0x32000be1),
        (0x3c890, 0x97ffdbdc),
        (0x3c898, 0x97fff52e),
        (0x3c8ac, 0x940099a5),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup hierarchy {a:x}"
        );
    }
    eprintln!(
        "Original Bustup catalog:{} backgrounds,{} characters,{outfits} outfits,{poses} poses,{expressions} expression references",
        catalog.backgrounds.len(),
        catalog.characters.len()
    );
}

#[test]
fn end_of_sized_section_closes_last_nested_definition() {
    let c = BustupCatalog::new(&vec![bg(3, 0), EndDefinition, b(7), b(8)]).unwrap();
    assert_eq!(c.characters.len(), 1);
    assert_eq!(c.characters[0].outfits.len(), 1);
    assert_eq!(
        c.characters[0].outfits[0].poses[0].expressions,
        [BustupId::from_number(7), BustupId::from_number(8)]
    );
}
