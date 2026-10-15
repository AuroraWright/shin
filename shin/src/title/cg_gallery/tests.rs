use super::*;
use shin_core::format::scenario::{
    info::PictureBoxInfoItem, instruction_elements::FromNumber, types::U16SmallList,
};
use shin_core::format::text::U16String;

#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_cg_list_controls_tasks_and_authored_assets() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario, texture_archive::read_texture_archive},
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
        (0x10dc90, 0x97fcf850),
        (0x4e1f8, 0x97ffe3ba),
        (0x47258, 0xb0000ee8),
        (0x47334, 0xf94053e0),
        (0x47338, 0x940229ce),
        (0x47490, 0x360001d5),
        (0x474a0, 0xb942f3e8),
        (0x475e0, 0xb942b3e8),
        (0x47764, 0xb24007eb),
        (0x47a54, 0x1ad30d08),
        (0x4926c, 0x9401c9e1),
        (0x49280, 0x37e00328),
        (0x49284, 0x37e80368),
        (0x493ac, 0x90000888),
        (0x49420, 0xaa1303e0),
        (0x49820, 0x90000888),
        (0x4985c, 0x1e2e1000),
        (0x49904, 0x1e221001),
        (0x49914, 0x9401c83b),
        (0x4966c, 0xb9404108),
        (0x49684, 0x97ff770b),
        (0x49718, 0xf0000ec8),
        (0x4d1b4, 0x9401ba0f),
        (0x4d1e0, 0x97fff568),
        (0x4d024, 0x97ff9b03),
        (0x4d034, 0x940303db),
        (0x49b6c, 0x1e212809),
        (0x49b80, 0x1e282801),
        (0x49b88, 0x94020f22),
        (0x49c0c, 0x540000c4),
        (0x49c28, 0x11000500),
        (0x49cec, 0xb901b668),
        (0x49cf0, 0x97ffff84),
        (0x49d14, 0xf900de7f),
        (0x49e44, 0xf900d808),
        (0x49f30, 0x9401c6b4),
        (0x49f48, 0x17fffe1e),
        (0x49f94, 0x97ff74c7),
        (0x49fa8, 0x17fffd36),
        (0x4a038, 0x97fffde2),
        (0x4a04c, 0x17fffd65),
        (0x4a170, 0xb901ba7f),
        (0x4a180, 0x1e293901),
        (0x4a1c4, 0x97fff81b),
        (0x4a1d0, 0x9401c444),
        (0x4a218, 0xbd01bc01),
        (0x4a274, 0x9401c5e3),
        (0x4a2e0, 0x9401c5c8),
        (0x480d4, 0x1e280821),
        (0x480d8, 0x1e213809),
        (0x48120, 0x54000264),
        (0x48138, 0x9401cc6a),
        (0x4814c, 0x940352bd),
        (0x48150, 0x1e200920),
        (0x48168, 0x54000045),
        (0x4816c, 0xb901be7f),
        (0xcd810, 0xd65f03c0), // Coordinate conversion is a platform no-op.
    ] {
        assert_eq!(word(a), expected, "native CG instruction at {a:x}");
    }
    for (a, expected) in [
        (0x159910, 412.0f32),
        (0x159a28, 242.0),
        (0x159860, 726.0),
        (0x159414, -726.0),
        (0x159694, 32.0),
        (0x159974, 0.2),
        (0x1595a4, 0.7),
        (0x159658, 116.0),
        (0x159480, 1764.0),
        (0x15971c, 1848.0),
        (0x159a20, 150.0),
        (0x159944, 992.0),
        (0x159ab8, f32::EPSILON),
        (0x1599d8, 916.0),
        (0x159340, 136.0),
        (0x1595dc, 1784.0),
        (0x1595e4, 190.0),
        (0x159890, -136.0),
        (0x159728, -190.0),
        (0x1595d8, 0.95),
        (0x159570, 0.1),
    ] {
        assert_eq!(f32::from_bits(word(a)), expected);
    }
    assert_eq!(&exe[0x15c694 + 256..0x15c694 + 267], b"/cgmode.txa");
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let mut load = |path: &str| {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let scenario = Scenario::new(load("/main.snr").into()).unwrap();
    let atlas = read_texture_archive(&load("/cgmode.txa")).unwrap();
    for (name, size) in [
        ("bg", (1920, 1080)),
        ("logo", (384, 216)),
        ("num", (512, 80)),
        ("help_cgmode", (320, 192)),
        ("key", (384, 96)),
        ("close", (84, 84)),
    ] {
        assert_eq!(atlas.textures[atlas.name_to_index[name]].dimensions(), size);
    }
    let info = &scenario.info_tables().picture_box_info;
    assert_eq!(info.len(), 119);
    let mut save = SaveData::default();
    let locked = CgGalleryList::new(info, &save).unwrap();
    assert_eq!(locked.completion_tenths(), 0);
    assert!(!locked.all_unlocked());
    let mut variants = 0;
    for row in locked.rows() {
        assert_eq!(
            atlas.textures[atlas.name_to_index[&row.thumbnail]].dimensions(),
            (384, 216)
        );
        for id in &row.pictures {
            let path = scenario.info_tables().picture_info(*id).path();
            assert!(rom.find_file(&path).is_ok(), "CG picture {path}");
            save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
            variants += 1;
        }
    }
    let full = CgGalleryList::new(info, &save).unwrap();
    assert_eq!(variants, 299);
    assert_eq!(full.content_height(), 7260.0);
    assert!(full.all_unlocked());
    assert_eq!(full.completion_label(), "達成率:100％");
    eprintln!(
        "CG authored rows={}, ordered variants={}, content_height={}",
        info.len(),
        variants,
        full.content_height()
    );
}

fn entry(name: &str, ids: &[i32]) -> PictureBoxInfoItem {
    PictureBoxInfoItem {
        name: U16String::new(name),
        picture_ids: U16SmallList::from_contents(ids.iter().map(|id| PictureId::from_number(*id))),
    }
}
fn list(count: usize) -> CgGalleryList {
    let info = (0..count)
        .map(|i| entry(&format!("thumb{i}"), &[i as i32]))
        .collect();
    let mut save = SaveData::default();
    save.save_vectors.unlocked_cgs.add(0);
    CgGalleryList::new(&info, &save).unwrap()
}

#[test]
fn pointer_uses_full_plates_half_open_viewport_and_scroll_fraction() {
    use glam::vec2;
    let mut g = list(17);
    assert_eq!(g.pointer_hit(vec2(136.0, 190.0)), 1);
    assert_eq!(g.pointer_hit(vec2(547.999, 431.999)), 1);
    assert_eq!(g.pointer_hit(vec2(548.0, 432.0)), 6);
    for p in [
        vec2(135.999, 190.0),
        vec2(1784.0, 250.0),
        vec2(200.0, 916.0),
    ] {
        assert_eq!(g.pointer_hit(p), -1);
    }
    assert!(!g.pointer_probe(vec2(200.0, 160.0)));
    assert!(g.pointer_probe(vec2(10.0, 10.0)));
    g.pointer_release();
    assert!(!g.closing());
    g.analog_scroll(0.01);
    assert_eq!(g.pointer_hit(vec2(200.0, 431.7)), 5);
    g.analog_scroll(100.0);
    assert_eq!(g.pointer_hit(vec2(600.0, 800.0)), 0);
    assert!(g.pointer_probe(vec2(600.0, 800.0))); // Missing tail accepts a scroll contact.
}

#[test]
fn pointer_press_reveal_bypasses_controller_scroll_gate_and_release_uses_capture() {
    use glam::vec2;
    let mut g = list(17);
    g.navigate(1, 0);
    assert!(!g.controller_ready());
    assert!(g.pointer_probe(vec2(140.0, 200.0)));
    g.pointer_press();
    assert_eq!(g.selected(), 0);
    ticks(&mut g, 1.0);
    assert!((g.feedback(0).1 - 0.2).abs() < 0.000001);
    g.pointer_release();
    assert_eq!(g.take_sounds(), ["move", "decide"]);
    ticks(&mut g, 6.0);
    assert!(g.take_requests().is_empty());
    g.prepare_frame();
    assert_eq!(g.take_requests(), [CgRequest::OpenRow(0)]);
}

#[test]
fn drag_cancels_activation_fling_uses_signed_time_and_touch_stops_without_play() {
    use glam::vec2;
    let mut g = list(24);
    assert!(g.pointer_probe(vec2(200.0, 250.0)));
    g.pointer_press();
    ticks(&mut g, 1.0);
    g.pointer_drag(-40.0);
    assert_eq!(g.scroll(), 40.0);
    assert_eq!(g.selected(), 4);
    g.pointer_release();
    g.pointer_gesture(vec2(0.0, -10.0));
    g.pointer_reset();
    ticks(&mut g, 2.0);
    assert_eq!(g.scroll(), 60.0);
    assert!((g.velocity() + 10.0 * 0.95_f32.powf(2.0)).abs() < 0.00001);
    let before = g.feedback(0);
    let v = g.velocity();
    ticks(&mut g, -1.0);
    assert_eq!(g.scroll(), 60.0 + v);
    assert!((g.velocity() - v / 0.95).abs() < 0.00001);
    assert_eq!(g.feedback(0), before);
    assert!(g.pointer_probe(vec2(200.0, 250.0)));
    assert_eq!(g.velocity(), 0.0);
    g.pointer_press();
    g.pointer_release();
    assert!(g.take_requests().is_empty());
    assert!(g.take_sounds().is_empty());
}

#[test]
fn fling_zero_delta_boundary_decay_and_pointer_close_cancel() {
    use glam::vec2;
    let mut g = list(24);
    assert!(g.pointer_probe(vec2(200.0, 250.0)));
    g.pointer_gesture(vec2(0.0, -10.0));
    ticks(&mut g, 0.0);
    assert_eq!(g.velocity(), 0.0);
    g.pointer_gesture(vec2(0.0, 10.0));
    ticks(&mut g, 1.0);
    assert_eq!(g.velocity(), 0.0);
    g.pointer_gesture(vec2(0.0, -0.101));
    ticks(&mut g, 1.0);
    assert_eq!(g.velocity(), 0.0);
    assert!(g.pointer_probe(vec2(1800.0, 80.0)));
    g.pointer_press();
    ticks(&mut g, 1.0);
    assert!((g.close_flash() - 0.2).abs() < 0.000001);
    g.pointer_drag(10.0);
    g.pointer_release();
    assert!(!g.closing());
    ticks(&mut g, 1.0);
    assert_eq!(g.close_flash(), 0.0);
    assert!(g.pointer_probe(vec2(1800.0, 80.0)));
    g.pointer_release();
    assert!(g.closing());
    assert_eq!(g.take_sounds(), ["decide"]);
}

fn ticks(g: &mut CgGalleryList, n: f32) {
    g.update(Ticks::from_f32(n));
}
#[test]
fn completion_counts_ordered_variants_and_duplicate_picture_references() {
    let info = vec![
        entry("many", &[0, 1, 2]),
        entry("one", &[3]),
        entry("duplicate", &[0]),
    ];
    let mut save = SaveData::default();
    save.save_vectors.unlocked_cgs.add(0);
    save.save_vectors.unlocked_cgs.add(90);
    let g = CgGalleryList::new(&info, &save).unwrap();
    assert_eq!(g.completion_label(), "達成率:40.0％");
    assert!(!g.all_unlocked());
    assert_eq!(
        g.rows()[0]
            .pictures
            .iter()
            .map(|id| id.into_i32())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(g.rows()[0].unlocked, [true, false, false]);
    assert_eq!(g.rows()[2].unlocked_count(), 1);
    assert_eq!(g.rows()[2].thumbnail, "duplicate");
    assert_eq!(g.feedback(0), (1.0, 0.0));
    for id in 1..4 {
        save.save_vectors.unlocked_cgs.add(id);
    }
    let full = CgGalleryList::new(&info, &save).unwrap();
    assert!(full.all_unlocked());
    assert_eq!(full.completion_label(), "達成率:100％");
    // Existing gallery is a construction snapshot of saved unlock bits.
    assert_eq!(g.completion_tenths(), 400);
    assert!(CgGalleryList::new(&vec![], &save).is_err());
    let empty = CgGalleryList::new(&vec![entry("empty", &[])], &save).unwrap();
    assert_eq!(empty.completion_tenths(), 0);
    assert!(empty.all_unlocked());
}
#[test]
fn navigation_clamps_tail_and_preserves_three_row_viewport_with_four_tick_gate() {
    let mut g = list(17);
    assert_eq!(g.content_height(), 1210.0);
    g.navigate(3, 1);
    assert_eq!(g.selected(), 7);
    assert_eq!(g.scroll(), 0.0);
    assert!(!g.controller_ready());
    g.controller(
        CgControls {
            cancel: true,
            confirm: true,
            ..Default::default()
        },
        0.0,
    );
    assert!(!g.closing());
    assert_eq!(g.take_sounds(), ["move"]);
    ticks(&mut g, 4.0);
    g.navigate(0, 1);
    assert_eq!(g.selected(), 11);
    ticks(&mut g, 4.0);
    g.navigate(0, 1);
    assert_eq!(g.selected(), 15);
    ticks(&mut g, 2.0);
    assert_eq!(g.scroll(), 121.0);
    g.navigate(0, 1);
    assert_eq!(g.selected(), 15);
    ticks(&mut g, 2.0);
    g.navigate(0, 1);
    assert_eq!(g.selected(), 16);
    ticks(&mut g, 4.0);
    assert_eq!(g.scroll(), 484.0);
    g.navigate(0, -1);
    assert_eq!(g.selected(), 12);
    ticks(&mut g, 4.0);
    assert_eq!(g.scroll(), 484.0);
    g.navigate(0, -3);
    ticks(&mut g, 4.0);
    assert_eq!(g.selected(), 0);
    assert_eq!(g.scroll(), 0.0);
    assert_eq!(list(2).content_height(), 726.0);
}
#[test]
fn fractional_analog_scroll_moves_focus_silently_and_keeps_column() {
    let mut g = list(17);
    g.analog_scroll(0.01);
    assert!((g.scroll() - 0.32).abs() < 0.000001);
    assert_eq!(g.selected(), 4);
    assert!(g.take_sounds().is_empty());
    assert!(g.controller_ready());
    g.analog_scroll(100.0);
    assert_eq!(g.scroll(), 484.0);
    assert_eq!(g.selected(), 8);
    g.analog_scroll(-100.0);
    assert_eq!(g.scroll(), 0.0);
    assert_eq!(g.selected(), 8);
    g.select(11, false);
    g.analog_scroll(100.0);
    assert_eq!(g.selected(), 11);
    g.analog_scroll(f32::NAN);
    assert_eq!(g.scroll(), 484.0);
}
#[test]
fn trackpad_wheel_scrolls_picture_gallery_through_live_input_dispatch() {
    let mut gallery = list(17);
    let mut input = super::super::cg_gallery_input::CgGalleryInput::new();
    for (number, (amount, expected)) in [
        (-3.25, 104.0),
        (-0.125, 108.0),
        (3.25, 4.0),
        (0.0, 4.0),
        (100.0, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let (actions, scroll) = crate::app::wheel_scroll_test_input(amount);
        input.input_frame(
            &mut gallery,
            true,
            &crate::adv::input_area::PointerFrame {
                number: number as u32,
                time_seconds: number as f64 / 60.0,
                events: &[],
            },
            &actions,
            scroll,
        );
        assert_eq!(gallery.scroll(), expected);
        assert!(gallery.take_sounds().is_empty());
        assert!(!gallery.closing());
    }
}
#[test]
fn confirm_preempts_cancel_and_navigation_while_locked_rows_still_flash() {
    let mut g = list(4);
    g.controller(
        CgControls {
            confirm: true,
            cancel: true,
            right: true,
            ..Default::default()
        },
        1.0,
    );
    assert_eq!(g.selected(), 0);
    assert!(!g.closing());
    assert_eq!(g.take_sounds(), ["decide"]);
    ticks(&mut g, 7.0);
    assert!(g.take_requests().is_empty());
    g.prepare_frame();
    assert_eq!(g.take_requests(), [CgRequest::OpenRow(0)]);
    ticks(&mut g, 0.0);
    g.select(1, false);
    g.play();
    assert_eq!(g.take_sounds(), ["error"]);
    ticks(&mut g, 1.0);
    assert!((g.feedback(1).1 - 0.2).abs() < 0.000001);
    ticks(&mut g, 6.0);
    g.prepare_frame();
    assert!(g.take_requests().is_empty());
}
#[test]
fn deferred_viewer_reads_live_selection_and_close_waits_next_poll() {
    let mut g = list(3);
    g.play();
    g.play();
    ticks(&mut g, 7.0);
    assert!(g.take_requests().is_empty());
    g.select(2, false);
    g.prepare_frame();
    assert_eq!(
        g.take_requests(),
        [CgRequest::OpenRow(2), CgRequest::OpenRow(2)]
    );
    ticks(&mut g, 0.0);
    g.controller(
        CgControls {
            cancel: true,
            ..Default::default()
        },
        0.0,
    );
    assert!(g.closing());
    assert_eq!(g.take_sounds(), ["decide", "decide", "cancel"]);
    ticks(&mut g, 7.0);
    assert!(g.take_requests().is_empty());
    g.prepare_frame();
    assert_eq!(
        g.take_requests(),
        [CgRequest::ReturnTitle {
            argument: 6,
            music_stop_ticks: 30
        }]
    );
}
#[test]
fn direction_priority_and_nonpositive_clocks_match_native_task_order() {
    let mut g = list(24);
    g.select(10, false);
    g.controller(
        CgControls {
            left: true,
            right: true,
            up: true,
            down: true,
            page_up: true,
            page_down: true,
            ..Default::default()
        },
        0.0,
    );
    assert_eq!(g.selected(), 5);
    let before = g.feedback(10);
    ticks(&mut g, 0.0);
    ticks(&mut g, -2.0);
    assert_eq!(g.feedback(10), before);
    assert!(!g.controller_ready());
    ticks(&mut g, 4.0);
    g.controller(
        CgControls {
            page_down: true,
            ..Default::default()
        },
        0.0,
    );
    assert_eq!(g.selected(), 17);
    ticks(&mut g, 4.0);
    assert_eq!(g.scroll(), 484.0);
}
