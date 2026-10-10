use super::*;
use shin_core::format::scenario::instruction_elements::FromNumber;
fn row(ids: &[i32], unlocked: &[bool]) -> CgRow {
    CgRow {
        entry: 0,
        thumbnail: "test".into(),
        pictures: ids.iter().map(|id| PictureId::from_number(*id)).collect(),
        unlocked: unlocked.into(),
        plate: [0.0, 0.0, 412.0, 242.0],
    }
}
fn ticks(m: &mut CgViewerNavigation, n: f32) {
    m.update(Ticks::from_f32(n));
}
#[test]
fn cursor_skips_locked_slots_preserves_duplicates_and_closes_after_last() {
    let r = row(&[10, 11, 10, 13, 14], &[false, true, true, false, true]);
    let mut m = CgViewerNavigation::new(&r);
    assert_eq!(m.ordinal(), 1);
    assert_eq!(
        m.take_requests(),
        [ViewerRequest::Load {
            ordinal: 1,
            picture: PictureId::from_number(11)
        }]
    );
    assert!(m.take_sounds().is_empty());
    m.pointer_release(&r);
    assert_eq!(
        m.take_requests(),
        [ViewerRequest::Load {
            ordinal: 2,
            picture: PictureId::from_number(10)
        }]
    );
    m.controller(
        &r,
        ViewerControls {
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(m.ordinal(), 4);
    assert_eq!(
        m.take_requests(),
        [ViewerRequest::Load {
            ordinal: 4,
            picture: PictureId::from_number(14)
        }]
    );
    m.pointer_release(&r);
    assert_eq!(m.ordinal(), -1);
    assert!(m.closing());
    assert!(!m.indicators());
    assert_eq!(m.take_requests(), [ViewerRequest::Close]);
    assert_eq!(m.take_sounds(), ["decide", "decide", "decide"]);
}
#[test]
fn blank_constructor_and_live_row_during_rapid_loading_confirmations() {
    let r = row(&[1, 2, 3], &[false, false, false]);
    let mut m = CgViewerNavigation::new(&r);
    assert_eq!(m.ordinal(), -1);
    assert!(!m.closing());
    assert!(m.take_requests().is_empty());
    // Constructor ignores next(false); input confirmation closes the blank child.
    m.controller(
        &r,
        ViewerControls {
            confirm: true,
            ..Default::default()
        },
    );
    assert!(m.closing());
    assert_eq!(m.take_requests(), [ViewerRequest::Close]);
    let original = row(&[4, 5, 6], &[true, false, true]);
    let changed = row(&[7, 8, 9, 9], &[false, true, false, true]);
    let mut m = CgViewerNavigation::new(&original);
    m.controller(
        &changed,
        ViewerControls {
            confirm: true,
            ..Default::default()
        },
    );
    m.controller(
        &changed,
        ViewerControls {
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(m.ordinal(), 3);
    assert_eq!(
        m.take_requests(),
        [
            ViewerRequest::Load {
                ordinal: 0,
                picture: PictureId::from_number(4)
            },
            ViewerRequest::Load {
                ordinal: 1,
                picture: PictureId::from_number(8)
            },
            ViewerRequest::Load {
                ordinal: 3,
                picture: PictureId::from_number(9)
            },
        ]
    );
    assert!(!m.closing()); // Publication is independent of navigation requests.
}
#[test]
fn cancel_precedes_confirm_fit_precedes_indicator_and_publication_restores_dots() {
    let r = row(&[1, 2], &[true, true]);
    let mut m = CgViewerNavigation::new(&r);
    m.take_requests();
    m.controller(
        &r,
        ViewerControls {
            fit_toggle: true,
            indicator_toggle: true,
            ..Default::default()
        },
    );
    assert!(!m.fit());
    assert!(m.indicators()); // No picture means no fit toggle.
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    m.controller(
        &r,
        ViewerControls {
            fit_toggle: true,
            indicator_toggle: true,
            ..Default::default()
        },
    );
    assert!(m.fit());
    assert!(m.indicators());
    m.controller(
        &r,
        ViewerControls {
            indicator_toggle: true,
            ..Default::default()
        },
    );
    assert!(!m.indicators());
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    assert!(m.indicators());
    m.controller(
        &r,
        ViewerControls {
            cancel: true,
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(m.take_requests(), [ViewerRequest::Close]);
    assert_eq!(m.take_sounds(), ["cancel"]);
    assert!(!m.pan_ready());
}
#[test]
fn native_pan_clamps_half_picture_fit_queue_and_pointer_bypasses_scale_gate() {
    let r = row(&[1], &[true]);
    let mut m = CgViewerNavigation::new(&r);
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    m.controller(
        &r,
        ViewerControls {
            pan: Vec2::new(2000.0, 1000.0),
            ..Default::default()
        },
    );
    assert_eq!(m.position(), Vec2::new(960.0, 540.0));
    assert_eq!(m.transform(), Vec4::new(960.0, -540.0, 1000.0, 1000.0));
    m.controller(
        &r,
        ViewerControls {
            fit_toggle: true,
            ..Default::default()
        },
    );
    ticks(&mut m, 5.0);
    assert_eq!(m.transform(), Vec4::new(480.0, -270.0, 750.0, 750.0));
    ticks(&mut m, -2.0);
    assert_eq!(m.transform().z, 750.0);
    m.controller(
        &r,
        ViewerControls {
            fit_toggle: true,
            ..Default::default()
        },
    );
    assert!(!m.fit());
    assert!(!m.pan_ready());
    m.controller(
        &r,
        ViewerControls {
            pan: Vec2::new(-20.0, 0.0),
            ..Default::default()
        },
    );
    assert_eq!(m.position().x, 960.0);
    ticks(&mut m, 5.0);
    assert_eq!(m.transform().z, 500.0); // Queued second toggle.
    m.pointer_drag(Vec2::new(-160.0, 160.0));
    assert_eq!(m.position(), Vec2::new(800.0, 450.0));
    assert_eq!(m.transform(), Vec4::new(800.0, -450.0, 1000.0, 1000.0));
    assert!(m.pan_ready());
    m.publish_picture(Vec2::new(1000.0, 2000.0));
    assert_eq!(m.position(), Vec2::new(0.0, 450.0));
    m.controller(
        &r,
        ViewerControls {
            fit_toggle: true,
            ..Default::default()
        },
    );
    ticks(&mut m, 10.0);
    assert!((m.transform().z - 540.0).abs() < 0.0001);
}
#[test]
fn velocity_integrates_once_per_frame_with_signed_decay_and_no_boundary_stop() {
    let r = row(&[1], &[true]);
    let mut m = CgViewerNavigation::new(&r);
    m.publish_picture(Vec2::new(3840.0, 2160.0));
    m.pointer_gesture(Vec2::new(10.0, 16.0));
    assert_eq!(m.velocity(), Vec2::new(10.0, 9.0));
    ticks(&mut m, 2.0);
    assert_eq!(m.position(), Vec2::new(10.0, -9.0));
    let v = m.velocity();
    ticks(&mut m, 0.0);
    assert_eq!(m.position(), Vec2::new(10.0, -9.0) + Vec2::new(v.x, -v.y));
    assert_eq!(m.velocity(), v);
    ticks(&mut m, -1.0);
    assert!((m.velocity().x - v.x / 0.95).abs() < 0.00001);
    m.pointer_drag(Vec2::new(10000.0, 0.0));
    ticks(&mut m, 1.0);
    assert_eq!(m.position().x, 960.0);
    assert!(m.velocity().x > 0.0);
    assert!(m.pointer_probe());
    assert_eq!(m.velocity(), Vec2::ZERO);
    m.pointer_gesture(Vec2::new(10.0, 0.0));
    m.controller(
        &r,
        ViewerControls {
            fit_toggle: true,
            ..Default::default()
        },
    );
    assert_eq!(m.velocity(), Vec2::ZERO);
    m.pointer_gesture(Vec2::new(10.0, 0.0));
    assert_eq!(m.velocity(), Vec2::ZERO);
}

#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_cg_variant_cursor_controls_placement_and_pan() {
    use shin_core::{
        format::{rom::RomReader, save::SaveData, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
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
        (0x4a80c, 0x790182a8),
        (0x4a810, 0xb900c6a9),
        (0x4a8d4, 0x940000ab),
        (0x4aa18, 0x37e803e8),
        (0x4aa1c, 0x37e00488),
        (0x4aa20, 0x373006a8),
        (0x4aa30, 0x39030668),
        (0x4aa48, 0x9400acb6),
        (0x4aa54, 0x39430a68),
        (0x4aac4, 0x9400002f),
        (0x4aad0, 0x94000360),
        (0x4ab10, 0x39030a68),
        (0x4ab18, 0xf900667f),
        (0x4ab68, 0xf900667f),
        (0x4abb4, 0xf900641f),
        (0x4abd8, 0xb900c66b),
        (0x4abec, 0xeb2dc17f),
        (0x4ac10, 0x54fffe40),
        (0x4ac28, 0x78697901),
        (0x4ac78, 0x12800008),
        (0x4b1b4, 0x9402d3bf),
        (0x4b1f4, 0x34000248),
        (0x4b23c, 0x2d59026a),
        (0x4b254, 0x2d1a0261),
        (0x4b260, 0x94034678),
        (0x4b278, 0x94034672),
        (0x4b2c4, 0x9401ead7),
        (0x4b2f8, 0x1e21cc00),
        (0x4b300, 0x1e201828),
        (0x4b320, 0x9401c1b8),
        (0x4b344, 0x9401c1af),
        (0x4b368, 0x9401c1a6),
        (0x4b3bc, 0x54000104),
        (0x4b3dc, 0xbd00d266),
        (0x4b408, 0xbd00d664),
        (0x4b434, 0x9401c173),
        (0x4b458, 0x9401c16a),
        (0x4b480, 0x9401c160),
        (0x4b4a0, 0x9401c158),
        (0x4b4ac, 0x9401bf8d),
        (0x4b4dc, 0x9401bf81),
        (0x4bb90, 0xf900641f),
        (0x4bc18, 0x97fffbda),
        (0x4bccc, 0x2d5a1002),
        (0x4bce0, 0x1e210800),
        (0x4bcf0, 0x17fffd6c),
        (0x4bd6c, 0x1e220821),
        (0x4bd70, 0x2d190400),
    ] {
        assert_eq!(word(a), expected, "native viewer at {a:x}");
    }
    for (a, expected) in [
        (0x159578, 1000.0f32),
        (0x159624, 960.0),
        (0x15954c, 540.0),
        (0x159888, -960.0),
        (0x15988c, -540.0),
        (0x1595d8, 0.95),
    ] {
        assert_eq!(f32::from_bits(word(a)), expected);
    }
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let info = &scenario.info_tables().picture_box_info;
    assert_eq!(info.len(), 119);
    for mode in 0..3 {
        let mut save = SaveData::default();
        for row in info {
            for id in &row.picture_ids.0 {
                let id = id.into_i32() as usize;
                if mode == 2 || (mode == 1 && id % 3 == 0) {
                    save.save_vectors.unlocked_cgs.add(id);
                }
            }
        }
        let gallery = super::super::cg_gallery::CgGalleryList::new(info, &save).unwrap();
        for row in gallery.rows() {
            let expected = row
                .pictures
                .iter()
                .enumerate()
                .filter(|(i, _)| row.unlocked[*i])
                .map(|(ordinal, picture)| ViewerRequest::Load {
                    ordinal,
                    picture: *picture,
                })
                .collect::<Vec<_>>();
            let mut model = CgViewerNavigation::new(row);
            if expected.is_empty() {
                assert!(!model.closing());
                assert!(model.take_requests().is_empty());
                model.pointer_release(row);
            } else {
                for (i, request) in expected.iter().enumerate() {
                    assert_eq!(model.take_requests(), [*request]);
                    assert_eq!(
                        model.ordinal(),
                        match request {
                            ViewerRequest::Load { ordinal, .. } => *ordinal as i32,
                            _ => unreachable!(),
                        }
                    );
                    if i % 2 == 0 {
                        model.pointer_release(row);
                    } else {
                        model.controller(
                            row,
                            ViewerControls {
                                confirm: true,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            assert!(model.closing());
            assert_eq!(model.ordinal(), -1);
            assert_eq!(model.take_requests(), [ViewerRequest::Close]);
        }
    }
}
