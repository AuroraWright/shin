use super::super::cg_viewer::ViewerControls;
use super::*;
use shin_core::format::scenario::{info::PictureId, instruction_elements::FromNumber};
fn row(unlocked: &[bool]) -> CgRow {
    CgRow {
        entry: 0,
        thumbnail: "test".into(),
        pictures: (0..unlocked.len())
            .map(|i| PictureId::from_number(i as i32))
            .collect(),
        unlocked: unlocked.into(),
        plate: [0.0, 0.0, 412.0, 242.0],
    }
}
#[test]
fn original_ordinals_are_shown_during_loading_including_locked_slots() {
    let r = row(&[false, true, false, true]);
    let mut m = CgViewerNavigation::new(&r);
    let marks = indicators(&m, &r);
    assert_eq!(marks.len(), 4);
    assert_eq!(
        marks[0],
        (
            vec4(1785.0, 1025.0, 1817.0, 1065.0),
            UnormColor::from_rgba(128, 128, 128, 255)
        )
    );
    assert_eq!(marks[1].1, UnormColor::from_rgba(144, 255, 128, 255));
    assert_eq!(
        marks[3],
        (vec4(1875.0, 1025.0, 1907.0, 1065.0), UnormColor::WHITE)
    );
    m.pointer_release(&r);
    assert_eq!(
        indicators(&m, &r)[3].1,
        UnormColor::from_rgba(144, 255, 128, 255)
    );
    assert_eq!(indicators(&m, &r)[1].1, UnormColor::WHITE);
    m.pointer_release(&r);
    assert!(indicators(&m, &r).is_empty());
}
#[test]
fn single_variants_hide_and_selected_color_precedes_live_locked_bits() {
    let single = row(&[true]);
    assert!(indicators(&CgViewerNavigation::new(&single), &single).is_empty());
    let r = row(&[true, true]);
    let mut m = CgViewerNavigation::new(&r);
    let changed = row(&[false, false]);
    assert_eq!(
        indicators(&m, &changed)[0].1,
        UnormColor::from_rgba(144, 255, 128, 255)
    );
    m.controller(
        &r,
        ViewerControls {
            indicator_toggle: true,
            ..Default::default()
        },
    );
    assert!(indicators(&m, &r).is_empty());
    m.publish_picture(glam::vec2(1920.0, 1080.0));
    assert_eq!(indicators(&m, &r).len(), 2);
    let blank = CgViewerNavigation::new(&changed);
    assert!(
        indicators(&blank, &changed)
            .iter()
            .all(|(_, color)| *color == UnormColor::from_rgba(128, 128, 128, 255))
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_cg_variant_indicators_order_colors_and_geometry() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x4b6bc, 0x94005e2d),
        (0x4b6c0, 0x39430668),
        (0x4b6c4, 0x340009a8),
        (0x4b6d4, 0xb981a916),
        (0x4b700, 0xb9404514),
        (0x4b704, 0x71000a9f),
        (0x4b708, 0x5400078b),
        (0x4b71c, 0x9133d421),
        (0x4b770, 0x1b097e89),
        (0x4b778, 0x529ff218),
        (0x4b77c, 0x72bff018),
        (0x4b780, 0x111dc536),
        (0x4b788, 0x320003f9),
        (0x4b78c, 0x5290101a),
        (0x4b790, 0x72bff01a),
        (0x4b798, 0x1e2a2802),
        (0x4b7ac, 0xb940c668),
        (0x4b7b8, 0x54000100),
        (0x4b7d4, 0x5a9f0341),
        (0x4b7dc, 0x9401ed55),
        (0x4b7e4, 0x9401ed8b),
        (0x4b7ec, 0x11007ad6),
        (0x47ae4, 0xd2a88408),
        (0x47ae8, 0xf2e84408),
        (0x47aec, 0xb26003e9),
        (0x47af0, 0xa90c27e8),
        (0x47b44, 0x97ff5c9f),
        (0x47b60, 0xbd4a6d00),
        (0x47b68, 0xbd43e101),
        (0x47b70, 0xbd447902),
        (0x47b8c, 0x9402e585),
        (0x48bfc, 0xbd458500),
        (0x48c2c, 0x9402e169),
    ] {
        assert_eq!(word(a), expected, "native CG indicators at {a:x}");
    }
    for (a, expected) in [
        (0x159764, 1025.0f32),
        (0x15934c, 1065.0),
        (0x159694, 32.0),
        (0x159a6c, 47.0 / 255.0),
        (0x1593e0, 25.0 / 255.0),
        (0x159478, 20.0 / 255.0),
        (0x159584, 48.0),
    ] {
        assert_eq!(f32::from_bits(word(a)), expected);
    }
    assert_eq!(&exe[0x162cf5 + 256..0x162cf5 + 262], b"check\0");
}
